//! Persistent host-coherent uniform storage. Wgpu still owns and submits the
//! buffer; only disjoint, previously unused bytes are written by the CPU.
use std::ptr::NonNull;
use wgpu::hal::{api::Vulkan, vulkan};
use ash::vk;

pub(crate) struct MappedUniforms {
    device: wgpu::Device,
    buffer: wgpu::Buffer,
    ptr: NonNull<u8>,
    memory: vk::DeviceMemory,
}

// The backend owns this mapping exclusively. Access requires &mut self;
// recorded draws retain only the GPU buffer, never this pointer.
unsafe impl Send for MappedUniforms {}

impl MappedUniforms {
    pub fn new(device: &wgpu::Device, size: u64) -> Result<Self, String> {
        unsafe {
            let hal = device.as_hal::<Vulkan>().ok_or("not Vulkan")?;
            let device_raw = hal.raw_device();
            let raw = device_raw.create_buffer(&vk::BufferCreateInfo::default()
                .size(size).usage(vk::BufferUsageFlags::UNIFORM_BUFFER | vk::BufferUsageFlags::TRANSFER_DST)
                .sharing_mode(vk::SharingMode::EXCLUSIVE), None).map_err(|e| e.to_string())?;
            let requirements = device_raw.get_buffer_memory_requirements(raw);
            let properties = hal.shared_instance().raw_instance()
                .get_physical_device_memory_properties(hal.raw_physical_device());
            // CpuToGpu's default allocation can be uncached BAR memory. Small
            // constant writes there cost more than the upload they replace.
            // Require cached + coherent memory; otherwise retain staged uploads.
            let flags = vk::MemoryPropertyFlags::HOST_VISIBLE
                | vk::MemoryPropertyFlags::HOST_COHERENT | vk::MemoryPropertyFlags::HOST_CACHED;
            let memory_type = (0..properties.memory_type_count).find(|&i|
                requirements.memory_type_bits & (1 << i) != 0
                && properties.memory_types[i as usize].property_flags.contains(flags));
            let Some(memory_type) = memory_type else {
                device_raw.destroy_buffer(raw, None);
                return Err("cached coherent uniform memory unavailable".into());
            };
            let memory = match device_raw.allocate_memory(&vk::MemoryAllocateInfo::default()
                .allocation_size(requirements.size).memory_type_index(memory_type), None) {
                Ok(memory) => memory,
                Err(e) => { device_raw.destroy_buffer(raw, None); return Err(e.to_string()); }
            };
            if let Err(e) = device_raw.bind_buffer_memory(raw, memory, 0) {
                device_raw.destroy_buffer(raw, None);
                device_raw.free_memory(memory, None);
                return Err(e.to_string());
            }
            let ptr = match device_raw.map_memory(memory, 0, requirements.size, vk::MemoryMapFlags::empty()) {
                Ok(ptr) => NonNull::new(ptr.cast::<u8>()).expect("nonempty Vulkan mapping"),
                Err(e) => {
                    device_raw.destroy_buffer(raw, None);
                    device_raw.free_memory(memory, None);
                    return Err(e.to_string());
                }
            };
            std::ptr::write_bytes(ptr.as_ptr(), 0, size as usize);
            // Managed import gives allocation destruction to wgpu along with
            // the buffer, including all outstanding submission keepalives.
            let raw = vulkan::Buffer::from_raw_managed(raw, memory, 0, requirements.size);
            drop(hal);
            // SAFETY: same device, initialized and nonempty. Public mapping is
            // disabled; our exclusive tail allocator implements synchronization.
            let buffer = device.create_buffer_from_hal::<Vulkan>(raw, &wgpu::BufferDescriptor {
                label: Some("mapped constants"), size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            super::MAPPED_RINGS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(Self { device: device.clone(), buffer, ptr, memory })
        }
    }

    pub fn buffer(&self) -> wgpu::Buffer { self.buffer.clone() }

    /// Caller supplies a fresh, nonoverlapping allocation in this generation.
    pub unsafe fn range(&mut self, offset: u64, len: u64) -> &mut [u8] {
        assert!(offset <= self.buffer.size() && len <= self.buffer.size() - offset);
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr().add(offset as usize), len as usize) }
    }

    /// Called only after the ordered finisher has submitted all previous users.
    /// Queue submission alone is insufficient before overwriting mapped bytes.
    pub fn restart(&self) {
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("mapped uniform ring GPU fence");
        super::MAPPED_RESTARTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Drop for MappedUniforms {
    fn drop(&mut self) {
        // Unmapping does not destroy the allocation. Wgpu and submitted raw
        // pass keepalives retain it until all GPU users have completed.
        unsafe {
            if let Some(device) = self.device.as_hal::<Vulkan>() {
                device.raw_device().unmap_memory(self.memory);
            }
        }
    }
}
