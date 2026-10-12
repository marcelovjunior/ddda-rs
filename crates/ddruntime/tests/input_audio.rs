//! ABI and observable service contracts; no installed game/device required.
use ddnative_base::{mem::Mem, x86::Cpu};
use ddruntime::{
    audio,
    dispatch::{Binding, Registry},
    input, interfaces,
    memory::{GuestMemory, READ, WRITE},
    runtime, services, threads, user32,
};
use std::sync::Arc;

const STACK: u32 = 0x10010000;
const DATA: u32 = 0x10000;
fn setup() -> (GuestMemory, Arc<Registry>) {
    runtime::enter(Arc::new(runtime::Runtime::default()));
    let mut m = GuestMemory::new();
    m.map(DATA, 0x10000, READ | WRITE).unwrap();
    m.map(STACK - 0x10000, 0x10000, READ | WRITE).unwrap();
    m.map_lazy(threads::STACKS_BASE, threads::STACKS_SIZE, READ | WRITE)
        .unwrap();
    let mut r = Registry::new();
    services::install(&mut m, &mut r).unwrap();
    let mut tokens = interfaces::Tokens::default();
    input::install(&mut m, &mut r, &mut tokens).unwrap();
    audio::install(&mut m, &mut r, &mut tokens).unwrap();
    for (a, f) in [
        (0x1000, buffer_end as fn(&mut dyn Mem, &mut Cpu)),
        (0x1010, callback_one),
        (0x1020, callback_two),
        (0x1030, callback_three),
        (0x1040, keyboard_hook),
        (0x1050, enum_item),
        (0x1060, engine_pass),
        (0x1070, fx_query),
        (0x1080, fx_ref),
        (0x1090, fx_release),
        (0x10a0, fx_parameters),
        (0x10b0, xapo_query),
        (0x10c0, xapo_lock),
        (0x10d0, xapo_halve),
        (0x10e0, user32::def_window_proc),
    ] {
        r.insert(a, Binding::Service(f)).unwrap();
    }
    let r = Arc::new(r);
    threads::set_machine(m.share(), r.clone());
    r.install();
    (m, r)
}
fn frame(m: &mut dyn Mem, args: &[u32]) -> Cpu {
    let esp = STACK - 0x100;
    m.set_u32(esp, 0xdeadbeef);
    for (i, v) in args.iter().enumerate() {
        m.set_u32(esp + 4 + i as u32 * 4, *v);
    }
    Cpu {
        esp,
        ..Cpu::default()
    }
}
fn api(m: &mut dyn Mem, f: fn(&mut dyn Mem, &mut Cpu), args: &[u32]) -> u32 {
    let mut c = frame(m, args);
    let end = c.esp + 4 + 4 * args.len() as u32;
    f(m, &mut c);
    assert_eq!(c.esp, end, "stdcall cleanup");
    c.eax
}
fn method(m: &mut dyn Mem, r: &Registry, obj: u32, slot: u32, args: &[u32]) -> u32 {
    let mut all = vec![obj];
    all.extend(args);
    let target = m.u32(m.u32(obj) + 4 * slot);
    let mut c = frame(m, &all);
    let end = c.esp + 4 + 4 * all.len() as u32;
    r.call(m, &mut c, target).unwrap();
    assert_eq!(c.esp, end, "slot {slot} cleanup");
    c.eax
}
fn put_guid(m: &mut dyn Mem, p: u32, g: [u32; 4]) {
    for (i, x) in g.into_iter().enumerate() {
        m.set_u32(p + 4 * i as u32, x);
    }
}
fn callback_one(_m: &mut dyn Mem, c: &mut Cpu) {
    c.esp += 8;
}
fn callback_two(_m: &mut dyn Mem, c: &mut Cpu) {
    c.esp += 12;
}
fn callback_three(_m: &mut dyn Mem, c: &mut Cpu) {
    c.esp += 16;
}
fn buffer_end(m: &mut dyn Mem, c: &mut Cpu) {
    let ctx = m.u32(c.esp + 8);
    m.set_u32(DATA + 0x900, m.u32(DATA + 0x900) + 1);
    m.set_u32(DATA + 0x904, ctx);
    m.set_u32(DATA + 0x908, threads::current_tid());
    c.esp += 12;
}
fn keyboard_hook(m: &mut dyn Mem, c: &mut Cpu) {
    m.set_u32(DATA + 0x910, threads::current_tid());
    let p = m.u32(c.esp + 12);
    m.set_u32(DATA + 0x914, m.u32(p));
    c.eax = u32::from(m.u32(p) == 0x42);
    c.esp += 16;
}
fn sleep(m: &mut dyn Mem, ms: u32) {
    api(m, threads::sleep, &[ms]);
}

#[test]
fn xinput_disconnected_ports_packet_changes_and_rumble() {
    let (mut m, _) = setup();
    let host = input::connect();
    for port in 0..4 {
        m.set_u32(DATA, 0xaaaaaaaa);
        assert_eq!(api(&mut m, input::xinput_state, &[port, DATA]), 1167);
        assert_eq!(m.u32(DATA), 0xaaaaaaaa);
        assert_eq!(api(&mut m, input::xinput_set, &[port, DATA]), 1167);
        assert_eq!(api(&mut m, input::xinput_caps, &[port, 0, DATA]), 1167);
    }
    assert_eq!(api(&mut m, input::xinput_state, &[4, DATA]), 87);
    let pad = input::Pad {
        buttons: 0x1000,
        left_x: -200,
        ..Default::default()
    };
    host.events
        .send(input::Event::Pad {
            port: 2,
            state: Some(pad),
        })
        .unwrap();
    assert_eq!(api(&mut m, input::xinput_state, &[2, DATA]), 0);
    assert_eq!(m.u32(DATA), 1);
    assert_eq!(m.u16(DATA + 4), 0x1000);
    assert_eq!(m.u16(DATA + 8), (-200i16) as u16);
    host.events
        .send(input::Event::Pad {
            port: 2,
            state: Some(pad),
        })
        .unwrap();
    api(&mut m, input::xinput_state, &[2, DATA]);
    assert_eq!(m.u32(DATA), 1);
    assert_eq!(api(&mut m, input::xinput_caps, &[2, 1, DATA + 32]), 0);
    assert_eq!(m.u8(DATA + 32), 1);
    assert_eq!(m.u8(DATA + 33), 1);
    m.set_u16(DATA + 64, 100);
    m.set_u16(DATA + 66, 200);
    assert_eq!(api(&mut m, input::xinput_set, &[2, DATA + 64]), 0);
    assert_eq!(
        host.rumble.try_recv().unwrap(),
        input::Rumble {
            port: 2,
            left: 100,
            right: 200
        }
    );
    api(&mut m, input::xinput_enable, &[0]);
    assert_eq!(host.rumble.try_recv().unwrap().left, 0);
    api(&mut m, input::xinput_state, &[2, DATA]);
    assert_eq!(m.u16(DATA + 4), 0);
    api(&mut m, input::xinput_enable, &[1]);
    assert_eq!(host.rumble.try_recv().unwrap().right, 200);
    host.events
        .send(input::Event::Pad { port: 2, state: None })
        .unwrap();
    assert_eq!(api(&mut m, input::xinput_state, &[2, DATA]), 1167);
}
#[test]
fn keyboard_hook_order_queue_state_and_character_messages() {
    let (mut m, _) = setup();
    let host = input::connect();
    let hook = api(&mut m, input::set_hook, &[13, 0x1040, 0, 0]);
    assert_ne!(hook, 0);
    host.events
        .send(input::Event::Key {
            hwnd: 7,
            vk: 0x41,
            scan: 30,
            down: true,
            extended: false,
            system: false,
        })
        .unwrap();
    assert_eq!(api(&mut m, input::get_async_key_state, &[0x41]), 0x8001);
    assert_eq!(api(&mut m, input::get_async_key_state, &[0x41]), 0x8000);
    assert_eq!(m.u32(DATA + 0x910), services::MAIN_THREAD_ID);
    assert_eq!(m.u32(DATA + 0x914), 0x41);
    assert_eq!(api(&mut m, input::get_key_state, &[0x41]), 0);
    assert_eq!(api(&mut m, user32::peek_message, &[DATA, 0, 0, 0, 1]), 1);
    assert_eq!(m.u32(DATA + 4), 0x100);
    assert_eq!(api(&mut m, input::get_key_state, &[0x41]), 0x8000);
    host.events
        .send(input::Event::Key {
            hwnd: 7,
            vk: 0x42,
            scan: 48,
            down: true,
            extended: false,
            system: false,
        })
        .unwrap();
    assert_eq!(api(&mut m, input::get_async_key_state, &[0x42]), 0);
    assert_eq!(api(&mut m, user32::peek_message, &[DATA, 0, 0, 0, 1]), 0);
    host.events
        .send(input::Event::Character {
            hwnd: 7,
            utf16: 65,
            system: false,
        })
        .unwrap();
    assert_eq!(api(&mut m, user32::peek_message, &[DATA, 0, 0, 0, 1]), 1);
    assert_eq!(m.u32(DATA + 4), 0x102);
    assert_eq!(api(&mut m, input::unhook, &[hook]), 1);
    assert_eq!(api(&mut m, input::unhook, &[hook]), 0);
    assert_eq!(api(&mut m, input::call_next_hook, &[0, 0, 0, 0]), 0);
    assert_eq!(api(&mut m, input::get_keyboard_layout, &[0]) & 0xffff, 0x409);
    assert_eq!(api(&mut m, input::map_virtual_key, &[65, 0]), 30);
    assert_eq!(api(&mut m, input::map_virtual_key, &[30, 1]), 65);
    assert_eq!(api(&mut m, input::map_virtual_key, &[65, 2]), 65);
    assert_eq!(api(&mut m, input::imm_virtual_key, &[7]), 0xe5);
}
#[test]
fn raw_device_empty_queries_and_system_settings_round_trip() {
    let (mut m, _) = setup();
    m.set_u32(DATA, 99);
    assert_eq!(api(&mut m, input::raw_list, &[0, DATA, 8]), 0);
    assert_eq!(m.u32(DATA), 0);
    assert_eq!(api(&mut m, input::raw_list, &[0, DATA, 16]), u32::MAX);
    assert_eq!(api(&mut m, input::raw_info, &[1, 0x20000007, 0, DATA]), u32::MAX);
    for (action, size, set) in [(0x32, 24, 0x33), (0x34, 8, 0x35), (0x3a, 8, 0x3b)] {
        m.set_u32(DATA, size);
        assert_eq!(api(&mut m, input::system_parameters, &[action, size, DATA, 0]), 1);
        assert_eq!(m.u32(DATA + 4), 0);
        m.set_u32(DATA + 4, 4);
        assert_eq!(api(&mut m, input::system_parameters, &[set, size, DATA, 0]), 1);
        m.set_u32(DATA + 4, 0);
        api(&mut m, input::system_parameters, &[action, size, DATA, 0]);
        assert_eq!(m.u32(DATA + 4), 4);
    }
    assert_eq!(api(&mut m, input::system_parameters, &[3, 0, DATA, 0]), 1);
    m.set_u32(DATA + 8, 1);
    assert_eq!(api(&mut m, input::system_parameters, &[4, 0, DATA, 0]), 1);
    api(&mut m, input::system_parameters, &[3, 0, DATA, 0]);
    assert_eq!(m.u32(DATA + 8), 1);
    for (set, get) in [(0x5d, 0x5e), (0x11, 0x10)] {
        assert_eq!(api(&mut m, input::system_parameters, &[set, 1, 0, 0]), 1);
        api(&mut m, input::system_parameters, &[get, 0, DATA, 0]);
        assert_eq!(m.u32(DATA), 1);
    }
    assert_eq!(api(&mut m, input::system_parameters, &[0x6f, 0, 0, 0]), 1);
    assert_eq!(api(&mut m, input::system_parameters, &[0xffffffff, 0, 0, 0]), 0);
}
fn di(m: &mut dyn Mem) -> u32 {
    put_guid(m, DATA, input::IID_INPUT);
    assert_eq!(
        api(m, input::direct_input_create, &[1, 0x800, DATA, DATA + 32, 0]),
        0
    );
    m.u32(DATA + 32)
}
#[test]
fn direct_input_identity_disconnect_and_mouse_format() {
    let (mut m, r) = setup();
    let host = input::connect();
    let di = di(&mut m);
    put_guid(&mut m, DATA, input::MOUSE_GUID);
    assert_eq!(method(&mut m, &r, di, 3, &[DATA, DATA + 32, 0]), 0x80040154);
    assert_eq!(m.u32(DATA + 32), 0);
    assert_eq!(method(&mut m, &r, di, 4, &[4, 0x1020, 0, 1]), 0);
    host.events
        .send(input::Event::Devices {
            keyboard: true,
            mouse: true,
        })
        .unwrap();
    assert_eq!(method(&mut m, &r, di, 3, &[DATA, DATA + 32, 0]), 0);
    let mouse = m.u32(DATA + 32);
    assert_eq!(method(&mut m, &r, mouse, 9, &[20, DATA + 64]), 0x8007000c);
    // DIDEVICEFORMAT + three relative axes and eight buttons, like DIMOUSESTATE2.
    for (i, v) in [24, 16, 2, 20, 11, DATA + 0x200].into_iter().enumerate() {
        m.set_u32(DATA + 0x100 + 4 * i as u32, v);
    }
    for i in 0..11 {
        m.set_u32(DATA + 0x200 + 16 * i + 4, if i < 3 { i * 4 } else { i + 9 });
        m.set_u32(
            DATA + 0x200 + 16 * i + 8,
            if i < 3 { (i << 8) | 1 } else { ((i - 3) << 8) | 4 },
        );
    }
    assert_eq!(method(&mut m, &r, mouse, 11, &[DATA + 0x100]), 0);
    assert_eq!(method(&mut m, &r, mouse, 13, &[7, 10]), 0);
    assert_eq!(method(&mut m, &r, mouse, 7, &[]), 0);
    assert_eq!(method(&mut m, &r, mouse, 7, &[]), 1);
    host.events
        .send(input::Event::Mouse {
            hwnd: 7,
            x: 10,
            y: 20,
            dx: 3,
            dy: -2,
            wheel: 120,
            buttons: [true, false, false, false, false, false, false, false],
        })
        .unwrap();
    assert_eq!(method(&mut m, &r, mouse, 25, &[]), 0);
    assert_eq!(method(&mut m, &r, mouse, 9, &[20, DATA + 64]), 0);
    assert_eq!(m.u32(DATA + 64), 3);
    assert_eq!(m.u32(DATA + 68), (-2i32) as u32);
    assert_eq!(m.u32(DATA + 72), 120);
    assert_eq!(m.u8(DATA + 76), 128);
    method(&mut m, &r, mouse, 9, &[20, DATA + 64]);
    assert_eq!(m.u32(DATA + 64), 0);
    assert_eq!(m.u8(DATA + 76), 128);
    put_guid(&mut m, DATA, input::IID_DEVICE);
    assert_eq!(method(&mut m, &r, mouse, 0, &[DATA, DATA + 32]), 0);
    assert_eq!(m.u32(DATA + 32), mouse);
    assert_eq!(method(&mut m, &r, mouse, 2, &[]), 1);
    m.set_u32(DATA + 64, 44);
    assert_eq!(method(&mut m, &r, mouse, 3, &[DATA + 64]), 0);
    assert_eq!(m.u32(DATA + 76), 3);
    m.set_u32(DATA + 64, 580);
    assert_eq!(method(&mut m, &r, mouse, 15, &[DATA + 64]), 0);
    assert_eq!(method(&mut m, &r, mouse, 8, &[]), 0);
    assert_eq!(method(&mut m, &r, mouse, 2, &[]), 0);
    assert_eq!(method(&mut m, &r, di, 2, &[]), 0);
}
fn engine(m: &mut dyn Mem, r: &Registry) -> u32 {
    put_guid(m, DATA, audio::CLSID_ENGINE);
    put_guid(m, DATA + 16, audio::IID_ENGINE);
    assert_eq!(api(m, audio::co_create, &[DATA, 0, 1, DATA + 16, DATA + 32]), 0);
    let e = m.u32(DATA + 32);
    assert_eq!(method(m, r, e, 5, &[0, u32::MAX]), 0);
    e
}
fn source(m: &mut dyn Mem, r: &Registry, e: u32, cb: u32) -> u32 {
    let f = DATA + 0x100;
    m.set_u16(f, 1);
    m.set_u16(f + 2, 1);
    m.set_u32(f + 4, 48000);
    m.set_u32(f + 8, 96000);
    m.set_u16(f + 12, 2);
    m.set_u16(f + 14, 16);
    m.set_u16(f + 16, 0);
    assert_eq!(
        method(m, r, e, 8, &[DATA + 32, f, 8, 2.0f32.to_bits(), cb, 0, 0]),
        0
    );
    m.u32(DATA + 32)
}
fn buffer(m: &mut dyn Mem, context: u32, flags: u32) {
    for (i, v) in [flags, 960, DATA + 0x1000, 0, 0, 0, 0, 0, context]
        .into_iter()
        .enumerate()
    {
        m.set_u32(DATA + 0x200 + 4 * i as u32, v);
    }
}
fn callback_object(m: &mut dyn Mem) -> u32 {
    let o = DATA + 0x300;
    let v = DATA + 0x400;
    m.set_u32(o, v);
    for (i, target) in [0x1020, 0x1010, 0x1010, 0x1020, 0x1000, 0x1020, 0x1030]
        .into_iter()
        .enumerate()
    {
        m.set_u32(v + 4 * i as u32, target);
    }
    o
}
#[test]
fn xaudio_com_27_layout_buffers_real_time_and_worker_callbacks() {
    let (mut m, r) = setup();
    let output = audio::output();
    let e = engine(&mut m, &r);
    assert_eq!(method(&mut m, &r, e, 3, &[DATA + 32]), 0);
    assert_eq!(m.u32(DATA + 32), 1);
    assert_eq!(method(&mut m, &r, e, 4, &[0, DATA + 0x2000]), 0);
    assert_eq!(m.u32(DATA + 0x2000 + 1028 + 4), 48000);
    assert_eq!(method(&mut m, &r, e, 10, &[DATA + 32, 0, 0, 0, 0, 0]), 0);
    let master = m.u32(DATA + 32);
    method(&mut m, &r, master, 0, &[DATA + 64]);
    assert_eq!(m.u32(DATA + 68), 2);
    let cb = callback_object(&mut m);
    let voice = source(&mut m, &r, e, cb);
    buffer(&mut m, 0x1234, 0x40);
    assert_eq!(method(&mut m, &r, voice, 21, &[DATA + 0x200, 0]), 0);
    method(&mut m, &r, voice, 25, &[DATA + 64]);
    assert_eq!(m.u32(DATA + 68), 1);
    assert_eq!(m.u32(DATA + 64), 0, "queued but not processing");
    assert_eq!(m.u32(DATA + 0x900), 0);
    assert_eq!(method(&mut m, &r, voice, 19, &[0, 0]), 0);
    sleep(&mut m, 40);
    method(&mut m, &r, voice, 25, &[DATA + 64]);
    assert_eq!(m.u32(DATA + 68), 0);
    assert_eq!(m.u32(DATA + 0x900), 1);
    assert_eq!(m.u32(DATA + 0x904), 0x1234);
    assert_ne!(m.u32(DATA + 0x908), services::MAIN_THREAD_ID);
    let block = output.try_recv().unwrap();
    assert_eq!(
        (block.sample_rate, block.channels, block.samples.len()),
        (48000, 2, 960)
    );
    assert!(block.samples.iter().all(|x| *x == 0.0));
    method(&mut m, &r, voice, 18, &[]);
    method(&mut m, &r, master, 18, &[]);
    assert_eq!(method(&mut m, &r, e, 2, &[]), 0);
    sleep(&mut m, 15);
    assert!(threads::faults().is_empty(), "{:?}", threads::faults());
}
#[test]
fn xaudio_state_operation_sets_flush_stop_and_format_validation() {
    let (mut m, r) = setup();
    let e = engine(&mut m, &r);
    let cb = callback_object(&mut m);
    let v = source(&mut m, &r, e, cb);
    assert_eq!(method(&mut m, &r, v, 12, &[0.5f32.to_bits(), 17]), 0);
    method(&mut m, &r, v, 13, &[DATA + 64]);
    assert_eq!(m.u32(DATA + 64), 1.0f32.to_bits());
    assert_eq!(method(&mut m, &r, e, 13, &[17]), 0);
    method(&mut m, &r, v, 13, &[DATA + 64]);
    assert_eq!(m.u32(DATA + 64), 0.5f32.to_bits());
    assert_eq!(method(&mut m, &r, v, 26, &[2.0f32.to_bits(), 0]), 0);
    method(&mut m, &r, v, 27, &[DATA + 64]);
    assert_eq!(m.u32(DATA + 64), 2.0f32.to_bits());
    assert_ne!(method(&mut m, &r, v, 26, &[f32::NAN.to_bits(), 0]), 0);
    buffer(&mut m, 1, 0);
    assert_eq!(method(&mut m, &r, v, 21, &[DATA + 0x200, 0]), 0);
    assert_ne!(method(&mut m, &r, v, 28, &[44100]), 0);
    assert_eq!(method(&mut m, &r, v, 22, &[]), 0);
    sleep(&mut m, 30);
    assert_eq!(m.u32(DATA + 0x900), 1);
    assert_eq!(method(&mut m, &r, v, 28, &[44100]), 0);
    assert_eq!(method(&mut m, &r, v, 20, &[0, 0]), 0);
    method(&mut m, &r, v, 18, &[]);
    method(&mut m, &r, e, 2, &[]);
    sleep(&mut m, 15);
    assert!(threads::faults().is_empty());
}

fn enum_item(m: &mut dyn Mem, c: &mut Cpu) {
    let p = m.u32(c.esp + 4);
    let count = m.u32(c.esp + 8);
    assert!(matches!(m.u32(p), 316 | 580));
    m.set_u32(count, m.u32(count) + 1);
    c.eax = 1;
    c.esp += 12;
}
fn engine_pass(m: &mut dyn Mem, c: &mut Cpu) {
    m.set_u32(DATA + 0x920, m.u32(DATA + 0x920) + 1);
    m.set_u32(DATA + 0x924, threads::current_tid());
    c.esp += 8;
}
#[test]
fn direct_input_enumeration_properties_events_and_error_abis() {
    let (mut m, r) = setup();
    let host = input::connect();
    let di = di(&mut m);
    assert_eq!(method(&mut m, &r, di, 1, &[]), 2);
    assert_eq!(method(&mut m, &r, di, 2, &[]), 1);
    assert_eq!(method(&mut m, &r, di, 4, &[0, 0x1050, DATA + 0x900, 1]), 0);
    assert_eq!(m.u32(DATA + 0x900), 0);
    host.events
        .send(input::Event::Devices {
            keyboard: true,
            mouse: true,
        })
        .unwrap();
    assert_eq!(method(&mut m, &r, di, 4, &[0, 0x1050, DATA + 0x900, 1]), 0);
    assert_eq!(m.u32(DATA + 0x900), 2);
    put_guid(&mut m, DATA, input::MOUSE_GUID);
    assert_eq!(method(&mut m, &r, di, 5, &[DATA]), 0);
    assert_eq!(method(&mut m, &r, di, 7, &[0, 0x800]), 0);
    assert_eq!(method(&mut m, &r, di, 6, &[0, 0]), 0x80004001);
    assert_eq!(method(&mut m, &r, di, 8, &[DATA, 0, DATA + 32]), 0x80040154);
    method(&mut m, &r, di, 3, &[DATA, DATA + 32, 0]);
    let v = m.u32(DATA + 32);
    assert_eq!(method(&mut m, &r, v, 1, &[]), 2);
    method(&mut m, &r, v, 2, &[]);
    m.set_u32(DATA + 0x900, 0);
    assert_eq!(method(&mut m, &r, v, 4, &[0x1050, DATA + 0x900, 0]), 0);
    assert_eq!(m.u32(DATA + 0x900), 11);
    m.set_u32(DATA + 64, 316);
    assert_eq!(method(&mut m, &r, v, 14, &[DATA + 64, 4, 1]), 0);
    assert_eq!(m.u32(DATA + 64 + 24), 0x101);
    for slot in [5, 6] {
        assert_eq!(method(&mut m, &r, v, slot, &[4, DATA + 64]), 0x80004001);
    }
    assert_eq!(method(&mut m, &r, v, 10, &[20, 0, DATA + 32, 0]), 0x80040207);
    assert_eq!(m.u32(DATA + 32), 0);
    assert_eq!(method(&mut m, &r, v, 18, &[DATA, 0, DATA + 32, 0]), 0x80004001);
    assert_eq!(m.u32(DATA + 32), 0);
    assert_eq!(method(&mut m, &r, v, 19, &[0x1050, DATA + 0x900, 0]), 0);
    for slot in [21, 22] {
        assert_eq!(method(&mut m, &r, v, slot, &[DATA + 32]), 0x80004001);
    }
    // Original 0x0144B114 uses ANYINSTANCE, not explicit axis/button numbers.
    for (i, x) in [24, 16, 2, 20, 11, DATA + 0x200].into_iter().enumerate() {
        m.set_u32(DATA + 0x100 + i as u32 * 4, x);
    }
    for i in 0..11 {
        m.set_u32(DATA + 0x200 + i * 16 + 4, if i < 3 { i * 4 } else { i + 9 });
        m.set_u32(DATA + 0x200 + i * 16 + 8, if i < 3 { 0xffff03 } else { 0xffff0c });
    }
    assert_eq!(method(&mut m, &r, v, 11, &[DATA + 0x100]), 0);
    let event = api(&mut m, threads::create_event, &[0, 0, 0, 0]);
    assert_eq!(method(&mut m, &r, v, 12, &[event]), 0);
    method(&mut m, &r, v, 7, &[]);
    host.events
        .send(input::Event::Mouse {
            hwnd: 0,
            x: 1,
            y: 2,
            dx: 3,
            dy: 4,
            wheel: 5,
            buttons: [false; 8],
        })
        .unwrap();
    method(&mut m, &r, v, 9, &[20, DATA + 64]);
    assert_eq!((m.u32(DATA + 64), m.u32(DATA + 68), m.u32(DATA + 72)), (3, 4, 5));
    assert_eq!(threads::wait(&[event], false, 0), 0);
    host.events
        .send(input::Event::Devices {
            keyboard: true,
            mouse: false,
        })
        .unwrap();
    assert_eq!(method(&mut m, &r, v, 7, &[]), 0x8007001e);
    assert_eq!(method(&mut m, &r, v, 9, &[20, DATA + 64]), 0x8007000c);
    method(&mut m, &r, v, 2, &[]);
    method(&mut m, &r, di, 2, &[]);
}
#[test]
fn xaudio_all_control_slots_effects_routing_and_engine_callbacks() {
    let (mut m, r) = setup();
    let e = engine(&mut m, &r);
    put_guid(&mut m, DATA, audio::IID_ENGINE);
    assert_eq!(method(&mut m, &r, e, 0, &[DATA, DATA + 32]), 0);
    assert_eq!(m.u32(DATA + 32), e);
    assert_eq!(method(&mut m, &r, e, 1, &[]), 3);
    method(&mut m, &r, e, 2, &[]);
    method(&mut m, &r, e, 2, &[]);
    assert_ne!(method(&mut m, &r, e, 5, &[0, u32::MAX]), 0);
    assert_ne!(method(&mut m, &r, e, 4, &[1, DATA + 0x2000]), 0);
    method(&mut m, &r, e, 15, &[0, 0]);
    m.set_u32(DATA + 0x700, DATA + 0x710);
    m.set_u32(DATA + 0x710, 0x1060);
    m.set_u32(DATA + 0x714, 0x1060);
    m.set_u32(DATA + 0x718, 0x1020);
    assert_eq!(method(&mut m, &r, e, 6, &[DATA + 0x700]), 0);
    method(&mut m, &r, e, 10, &[DATA + 32, 0, 0, 0, 0, 0]);
    let master = m.u32(DATA + 32);
    assert_eq!(method(&mut m, &r, e, 9, &[DATA + 32, 1, 48000, 0, 1, 0, 0]), 0);
    let sub = m.u32(DATA + 32);
    let v = source(&mut m, &r, e, 0);
    m.set_u32(DATA + 0x500, 1);
    m.set_u32(DATA + 0x504, DATA + 0x510);
    m.set_u32(DATA + 0x510, 0x80);
    m.set_u32(DATA + 0x514, sub);
    assert_eq!(method(&mut m, &r, v, 1, &[DATA + 0x500]), 0);
    m.set_u32(DATA + 0x520, 0.25f32.to_bits());
    assert_eq!(method(&mut m, &r, v, 16, &[sub, 1, 1, DATA + 0x520, 0]), 0);
    method(&mut m, &r, v, 17, &[sub, 1, 1, DATA + 0x524]);
    assert_eq!(m.u32(DATA + 0x524), 0.25f32.to_bits());
    m.set_u32(DATA + 0x520, 0.75f32.to_bits());
    assert_eq!(method(&mut m, &r, v, 14, &[1, DATA + 0x520, 0]), 0);
    method(&mut m, &r, v, 15, &[1, DATA + 0x524]);
    assert_eq!(m.u32(DATA + 0x524), 0.75f32.to_bits());
    for (i, x) in [0, 0.5f32.to_bits(), 1.0f32.to_bits()].into_iter().enumerate() {
        m.set_u32(DATA + 0x540 + i as u32 * 4, x);
    }
    assert_eq!(method(&mut m, &r, v, 8, &[DATA + 0x540, 0]), 0);
    method(&mut m, &r, v, 9, &[DATA + 0x550]);
    assert_eq!(m.u32(DATA + 0x554), 0.5f32.to_bits());
    assert_eq!(method(&mut m, &r, v, 10, &[sub, DATA + 0x540, 0]), 0);
    method(&mut m, &r, v, 11, &[sub, DATA + 0x550]);
    assert_eq!(m.u32(DATA + 0x554), 0.5f32.to_bits());
    put_guid(&mut m, DATA, audio::CLSID_REVERB);
    put_guid(&mut m, DATA + 16, input::IUNKNOWN);
    assert_eq!(
        api(&mut m, audio::co_create, &[DATA, 0, 1, DATA + 16, DATA + 32]),
        0
    );
    let fx = m.u32(DATA + 32);
    assert_eq!(method(&mut m, &r, fx, 0, &[DATA + 16, DATA + 32]), 0);
    method(&mut m, &r, fx, 2, &[]);
    m.set_u32(DATA + 0x600, 1);
    m.set_u32(DATA + 0x604, DATA + 0x610);
    m.set_u32(DATA + 0x610, fx);
    m.set_u32(DATA + 0x614, 1);
    m.set_u32(DATA + 0x618, 1);
    assert_eq!(method(&mut m, &r, v, 2, &[DATA + 0x600]), 0);
    method(&mut m, &r, v, 4, &[0, 0]);
    method(&mut m, &r, v, 5, &[0, DATA + 64]);
    assert_eq!(m.u32(DATA + 64), 0);
    method(&mut m, &r, v, 3, &[0, 0]);
    method(&mut m, &r, v, 5, &[0, DATA + 64]);
    assert_eq!(m.u32(DATA + 64), 1);
    m.set_u32(DATA + 0x650, 42);
    assert_eq!(method(&mut m, &r, v, 6, &[0, DATA + 0x650, 52, 0]), 0);
    method(&mut m, &r, v, 7, &[0, DATA + 0x654, 52]);
    assert_eq!(m.u32(DATA + 0x654), 42);
    assert_eq!(method(&mut m, &r, fx, 2, &[]), 1);
    method(&mut m, &r, v, 2, &[0]);
    method(&mut m, &r, e, 14, &[DATA + 0x800]);
    assert_eq!(m.u32(DATA + 0x800 + 40), 1);
    sleep(&mut m, 30);
    assert!(m.u32(DATA + 0x920) > 0);
    assert_ne!(m.u32(DATA + 0x924), services::MAIN_THREAD_ID);
    method(&mut m, &r, e, 12, &[]);
    let count = m.u32(DATA + 0x920);
    sleep(&mut m, 20);
    assert_eq!(m.u32(DATA + 0x920), count);
    method(&mut m, &r, e, 11, &[]);
    method(&mut m, &r, e, 7, &[DATA + 0x700]);
    sleep(&mut m, 20);
    assert_eq!(m.u32(DATA + 0x920), count);
    for voice in [v, sub, master] {
        method(&mut m, &r, voice, 18, &[]);
    }
    method(&mut m, &r, e, 2, &[]);
    sleep(&mut m, 15);
    assert!(threads::faults().is_empty());
}
#[test]
fn xaudio_infinite_loop_exit_discontinuity_and_adpcm_clock() {
    let (mut m, r) = setup();
    let e = engine(&mut m, &r);
    let cb = callback_object(&mut m);
    let v = source(&mut m, &r, e, cb);
    buffer(&mut m, 123, 0);
    m.set_u32(DATA + 0x200 + 28, 255);
    assert_eq!(method(&mut m, &r, v, 21, &[DATA + 0x200, 0]), 0);
    method(&mut m, &r, v, 19, &[0, 0]);
    sleep(&mut m, 35);
    method(&mut m, &r, v, 25, &[DATA + 64]);
    assert_eq!(m.u32(DATA + 68), 1);
    assert!(m.u32(DATA + 72) >= 960);
    assert_eq!(m.u32(DATA + 0x900), 0);
    method(&mut m, &r, v, 23, &[]);
    method(&mut m, &r, v, 24, &[0]);
    sleep(&mut m, 25);
    method(&mut m, &r, v, 25, &[DATA + 64]);
    assert_eq!(m.u32(DATA + 68), 0);
    assert_eq!(m.u32(DATA + 72), 0);
    assert_eq!(m.u32(DATA + 0x900), 1);
    method(&mut m, &r, v, 18, &[]);
    // MS ADPCM: 128 samples in each 70-byte mono block (0x00DCE1F0).
    let f = DATA + 0x100;
    m.set_u16(f, 2);
    m.set_u16(f + 12, 70);
    m.set_u16(f + 14, 4);
    m.set_u16(f + 16, 32);
    m.set_u16(f + 18, 128);
    assert_eq!(
        method(&mut m, &r, e, 8, &[DATA + 32, f, 8, 2.0f32.to_bits(), cb, 0, 0]),
        0
    );
    let v = m.u32(DATA + 32);
    buffer(&mut m, 456, 0);
    m.set_u32(DATA + 0x204, 700);
    assert_eq!(method(&mut m, &r, v, 21, &[DATA + 0x200, 0]), 0);
    method(&mut m, &r, v, 19, &[0, 0]);
    sleep(&mut m, 40);
    method(&mut m, &r, v, 25, &[DATA + 64]);
    assert_eq!(m.u32(DATA + 68), 0);
    assert_eq!(m.u32(DATA + 72), 1280);
    assert_eq!(m.u32(DATA + 0x900), 2);
    method(&mut m, &r, e, 2, &[]);
    sleep(&mut m, 15);
    assert!(threads::faults().is_empty());
}

fn fx_query(m: &mut dyn Mem, c: &mut Cpu) {
    let this = m.u32(c.esp + 4);
    let iid = m.u32(c.esp + 8);
    let out = m.u32(c.esp + 12);
    // IXAPOParameters, 2.7 IID (game rdata 0x0142AF38)
    assert_eq!(m.u32(iid), 0xa90bc001);
    assert_eq!(m.u32(iid + 12), 0x01000000);
    m.set_u32(out, this);
    m.set_u32(DATA + 0xa00, m.u32(DATA + 0xa00) + 1);
    c.eax = 0;
    c.esp += 16;
}
fn fx_ref(m: &mut dyn Mem, c: &mut Cpu) {
    let refs = m.u32(DATA + 0xa00) + 1;
    m.set_u32(DATA + 0xa00, refs);
    c.eax = refs;
    c.esp += 8;
}
fn fx_release(m: &mut dyn Mem, c: &mut Cpu) {
    let refs = m.u32(DATA + 0xa00) - 1;
    m.set_u32(DATA + 0xa00, refs);
    c.eax = refs;
    c.esp += 8;
}
fn fx_parameters(m: &mut dyn Mem, c: &mut Cpu) {
    let p = m.u32(c.esp + 8);
    assert_eq!(m.u32(c.esp + 12), 4);
    m.set_u32(DATA + 0xa04, m.u32(p));
    m.set_u32(DATA + 0xa08, m.u32(DATA + 0xa08) + 1);
    c.esp += 16;
}
#[test]
fn xaudio_guest_xapo_parameters_and_deferred_effect_lifetime() {
    let (mut m, r) = setup();
    let e = engine(&mut m, &r);
    let v = source(&mut m, &r, e, 0);
    let fx = DATA + 0xb00;
    let vt = DATA + 0xb10;
    m.set_u32(fx, vt);
    m.set_u32(DATA + 0xa00, 1);
    for (i, t) in [0x1070, 0x1080, 0x1090, 0x10a0].into_iter().enumerate() {
        m.set_u32(vt + i as u32 * 4, t);
    }
    m.set_u32(DATA + 0xb30, 1);
    m.set_u32(DATA + 0xb34, DATA + 0xb40);
    m.set_u32(DATA + 0xb40, fx);
    m.set_u32(DATA + 0xb44, 1);
    m.set_u32(DATA + 0xb48, 1);
    assert_eq!(method(&mut m, &r, v, 2, &[DATA + 0xb30]), 0);
    assert_eq!(m.u32(DATA + 0xa00), 2);
    m.set_u32(DATA + 0xb60, 0x12345678);
    assert_eq!(method(&mut m, &r, v, 6, &[0, DATA + 0xb60, 4, 19]), 0);
    assert_eq!(m.u32(DATA + 0xa08), 0);
    method(&mut m, &r, e, 13, &[19]);
    assert_eq!(m.u32(DATA + 0xa08), 1);
    assert_eq!(m.u32(DATA + 0xa04), 0x12345678);
    assert_eq!(m.u32(DATA + 0xa00), 2);
    method(&mut m, &r, v, 6, &[0, DATA + 0xb60, 4, 20]);
    method(&mut m, &r, v, 2, &[0]);
    method(&mut m, &r, e, 13, &[20]);
    assert_eq!(m.u32(DATA + 0xa08), 1);
    assert_eq!(m.u32(DATA + 0xa00), 1);
    method(&mut m, &r, e, 2, &[]);
    sleep(&mut m, 15);
    assert!(threads::faults().is_empty());
}
#[test]
fn rejected_api_arguments_and_session_isolation() {
    let (mut m, _r) = setup();
    assert_eq!(api(&mut m, input::set_hook, &[3, 0x1040, 0, 0]), 0);
    assert_eq!(api(&mut m, input::call_next_hook, &[0, 0, 0, 0]), 0);
    assert_eq!(api(&mut m, input::unhook, &[123]), 0);
    put_guid(&mut m, DATA, input::IID_INPUT);
    assert_eq!(
        api(
            &mut m,
            input::direct_input_create,
            &[0, 0x700, DATA, DATA + 32, 0]
        ),
        0x80070481
    );
    put_guid(&mut m, DATA, audio::CLSID_ENGINE);
    put_guid(&mut m, DATA + 16, audio::IID_ENGINE);
    assert_eq!(
        api(&mut m, audio::co_create, &[DATA, 1, 1, DATA + 16, DATA + 32]),
        0x80040110
    );
    m.set_u32(DATA, 123);
    m.set_u32(DATA + 32, 123);
    assert_eq!(
        api(&mut m, audio::co_create, &[DATA, 0, 1, DATA + 16, DATA + 32]),
        0x80040154
    );
    assert_eq!(m.u32(DATA + 32), 0);
    let host = input::connect();
    host.events
        .send(input::Event::Pad {
            port: 0,
            state: Some(input::Pad::default()),
        })
        .unwrap();
    assert_eq!(api(&mut m, input::xinput_state, &[0, DATA]), 0);
    let (mut other, _other_r) = setup();
    assert_eq!(api(&mut other, input::xinput_state, &[0, DATA]), 1167);
}

#[test]
fn extended_modifiers_mouse_extra_buttons_and_screen_coordinates() {
    let (mut m, _r) = setup();
    let host = input::connect();
    host.events
        .send(input::Event::Key {
            hwnd: 7,
            vk: 0xa3,
            scan: 0x1d,
            down: true,
            extended: true,
            system: false,
        })
        .unwrap();
    assert_eq!(api(&mut m, input::get_async_key_state, &[0x11]), 0x8001);
    assert_eq!(api(&mut m, user32::peek_message, &[DATA, 0, 0, 0, 1]), 1);
    assert_eq!(m.u32(DATA + 8), 0x11);
    assert_ne!(m.u32(DATA + 12) & (1 << 24), 0);
    assert_eq!(api(&mut m, input::get_key_state, &[0x11]), 0x8000);
    assert_eq!(api(&mut m, input::get_key_state, &[0xa3]), 0x8000);
    assert_eq!(api(&mut m, input::get_key_state, &[0xa2]), 0);
    host.events
        .send(input::Event::Mouse {
            hwnd: 7,
            x: -3,
            y: 15,
            dx: 0,
            dy: 0,
            wheel: -120,
            buttons: [false, false, false, true, false, false, false, false],
        })
        .unwrap();
    assert_eq!(api(&mut m, user32::peek_message, &[DATA, 0, 0, 0, 1]), 1);
    assert_eq!(m.u32(DATA + 4), 0x200);
    assert_eq!(m.u32(DATA + 8), 0x28);
    assert_eq!(api(&mut m, user32::peek_message, &[DATA, 0, 0, 0, 1]), 1);
    assert_eq!(m.u32(DATA + 4), 0x20b);
    assert_eq!(m.u32(DATA + 8), 0x10028);
    assert_eq!(api(&mut m, user32::peek_message, &[DATA, 0, 0, 0, 1]), 1);
    assert_eq!(m.u32(DATA + 4), 0x20a);
    assert_eq!(m.u32(DATA + 8), ((-120i32 as u32) & 0xffff) << 16 | 0x28);
    assert_eq!(m.u32(DATA + 12), 0x000ffffd);
}

/// Private integration fixture: uses the player's exe, never checked-in bytes.
/// Run with DDDA_EXE set and `cargo test -p ddruntime --test input_audio -- --ignored`.
#[test]
#[ignore = "requires the player's DDDA.exe and the generated lifted program"]
fn original_engine_initialization() {
    use ddruntime::session::{Outcome, Session};
    let path = std::env::var("DDDA_EXE").expect("DDDA_EXE");
    let mut session = Session::new(&path).unwrap();
    session.mem.map(DATA, 0x10000, READ | WRITE).unwrap();
    session.mem.map(STACK - 0x10000, 0x10000, READ | WRITE).unwrap();
    let sound = api(
        &mut session.mem,
        ddruntime::win32::heap_alloc,
        &[ddruntime::win32::PROCESS_HEAP, 8, 0x51560],
    );
    assert_ne!(sound, 0);
    // Original function, not a translation in this test: engine + mastering voice.
    let result = session.call(0x00dce0e0, sound, &[]);
    assert!(
        matches!(&result,Outcome::Returned(c) if c.eax & 0xff == 1),
        "{result}"
    );
    let e = session.mem.u32(sound + 0x4dcf8);
    assert_ne!(e, 0);
    assert_ne!(session.mem.u32(sound + 0x4dcfc), 0);
    assert_eq!(session.mem.u32(sound + 0x4dcdc), 2);
    // Original OnBufferEnd at 0x00DCCD00 is absent from this inventory/build.
    // Do not replace it with a service: the head must add the missing native entry.
    method(&mut session.mem, &session.registry, e, 2, &[]);
    sleep(&mut session.mem, 15);
    assert!(threads::faults().is_empty(), "{:?}", threads::faults());
}

#[test]
fn all_input_audio_import_names_bind_including_xinput_ordinals() {
    let imports = input::APIS
        .iter()
        .chain(audio::APIS)
        .enumerate()
        .map(|(i, (dll, name, _))| ddruntime::image::Import {
            dll: dll.to_ascii_lowercase(),
            name: (*name).to_owned(),
            slot: 0,
            token: 0xf1000000 + i as u32 * 4,
        })
        .collect::<Vec<_>>();
    let mut r = Registry::new();
    assert_eq!(imports.len(), 17);
    assert_eq!(services::bind_apis(&mut r, &imports).unwrap(), 17);
    for i in imports {
        assert!(
            matches!(r.get(i.token), Some(Binding::Service(_))),
            "{}",
            i.full_name()
        );
    }
}

#[test]
fn host_input_without_a_window_handle_goes_to_the_focused_window() {
    let (mut m, _r) = setup();
    let host = input::connect();
    // a window like the game's: 1286x749 at (32, 32), style 0xCA0000, so its
    // client area starts at (35, 58) on the screen
    let (wc, name) = (DATA + 0xa00, DATA + 0xa80);
    m.set_u8(name, b'W');
    m.set_u32(wc, 0x30);
    m.set_u32(wc + 8, 0x10e0);
    m.set_u32(wc + 40, name);
    assert_ne!(api(&mut m, user32::register_class_ex_a, &[wc]), 0);
    let hwnd = api(&mut m, user32::create_window_ex_a, &[0, name, 0, 0x00CA_0000, 32, 32, 1286, 749, 0, 0, 0, 0]);
    assert_ne!(hwnd, 0);
    api(&mut m, user32::show_window, &[hwnd, 1]);
    // drain the start-up messages; dispatching WM_PAINT to DefWindowProc validates the window
    while api(&mut m, user32::peek_message, &[DATA, 0, 0, 0, 1]) != 0 {
        api(&mut m, user32::dispatch_message, &[DATA]);
    }
    host.events
        .send(input::Event::Key { hwnd: 0, vk: 0x42, scan: 48, down: true, extended: false, system: false })
        .unwrap();
    assert_eq!(api(&mut m, user32::peek_message, &[DATA, 0, 0, 0, 1]), 1);
    assert_eq!((m.u32(DATA), m.u32(DATA + 4), m.u32(DATA + 8)), (hwnd, 0x100, 0x42));
    host.events
        .send(input::Event::Mouse { hwnd: 0, x: 10, y: 20, dx: 0, dy: 0, wheel: 0, buttons: [false; 8] })
        .unwrap();
    assert_eq!(api(&mut m, user32::peek_message, &[DATA, 0, 0, 0, 1]), 1);
    assert_eq!((m.u32(DATA), m.u32(DATA + 4)), (hwnd, 0x200));
    assert_eq!(m.u32(DATA + 12), 20 << 16 | 10, "client coordinates as sent");
    let point = DATA + 0xb00;
    assert_eq!(api(&mut m, user32::get_cursor_pos, &[point]), 1);
    assert_eq!((m.u32(point), m.u32(point + 4)), (45, 78), "the cursor on the screen");

// --- audible output (brief L19) ----------------------------------------

fn take_output(output: &std::sync::mpsc::Receiver<audio::AudioBlock>) -> (Vec<f32>, Vec<f32>) {
    let (mut left, mut right) = (vec![], vec![]);
    while let Ok(b) = output.try_recv() {
        assert_eq!((b.sample_rate, b.channels), (48000, 2));
        for f in b.samples.chunks(2) {
            left.push(f[0]);
            right.push(f[1]);
        }
    }
    (left, right)
}
/// The frames of `want` appear, in order, from the first non-silent one.
fn assert_contains(got: &[f32], want: &[f32]) {
    let start = got.iter().position(|x| *x != 0.0).expect("audible output");
    assert_eq!(&got[start..start + want.len()], want);
}

#[test]
fn xaudio_mixes_submitted_pcm_into_the_output() {
    let (mut m, r) = setup();
    let output = audio::output();
    let e = engine(&mut m, &r);
    assert_eq!(method(&mut m, &r, e, 10, &[DATA + 32, 0, 0, 0, 0, 0]), 0);
    let master = m.u32(DATA + 32);
    let voice = source(&mut m, &r, e, 0);
    let pcm: Vec<i16> = (0..480).map(|i| (i * 37 - 9000) as i16).collect();
    for (i, x) in pcm.iter().enumerate() {
        m.set_u16(DATA + 0x1000 + 2 * i as u32, *x as u16);
    }
    buffer(&mut m, 0x1234, 0x40);
    assert_eq!(method(&mut m, &r, voice, 21, &[DATA + 0x200, 0]), 0);
    assert_eq!(method(&mut m, &r, voice, 19, &[0, 0]), 0);
    sleep(&mut m, 60);
    let (left, right) = take_output(&output);
    // 16-bit PCM is x / 32768; a mono voice's default send to a stereo
    // mastering voice feeds both channels at full scale.
    let want: Vec<f32> = pcm.iter().map(|x| *x as f32 / 32768.0).collect();
    assert_contains(&left, &want);
    assert_contains(&right, &want);
    method(&mut m, &r, voice, 18, &[]);
    method(&mut m, &r, master, 18, &[]);
    assert_eq!(method(&mut m, &r, e, 2, &[]), 0);
    sleep(&mut m, 15);
    assert!(threads::faults().is_empty(), "{:?}", threads::faults());
}

/// The bank voices' format, as 0x00DCE1F0 builds it.
fn game_adpcm_format(m: &mut dyn Mem, f: u32) {
    for (o, w) in [(0, 2u16), (2, 1), (12, 70), (14, 4), (16, 32), (18, 128), (20, 7)] {
        m.set_u16(f + o, w);
    }
    m.set_u32(f + 4, 48000);
    m.set_u32(f + 8, 0x668a);
    for (i, (a, b)) in [
        (256, 0),
        (512, -256),
        (0, 0),
        (192, 64),
        (240, 0),
        (460, -208),
        (392, -232),
    ]
    .into_iter()
    .enumerate()
    {
        m.set_u16(f + 22 + 4 * i as u32, a as i16 as u16);
        m.set_u16(f + 24 + 4 * i as u32, b as i16 as u16);
    }
}

#[test]
fn xaudio_decodes_the_games_adpcm_bank_format() {
    let (mut m, r) = setup();
    let output = audio::output();
    let e = engine(&mut m, &r);
    assert_eq!(method(&mut m, &r, e, 10, &[DATA + 32, 0, 0, 0, 0, 0]), 0);
    let f = DATA + 0x100;
    game_adpcm_format(&mut m, f);
    // 0x00DCE1F0's call: flags 0, max ratio 2, no callback, sends or effects.
    assert_eq!(
        method(&mut m, &r, e, 8, &[DATA + 32, f, 0, 2.0f32.to_bits(), 0, 0, 0]),
        0
    );
    let voice = m.u32(DATA + 32);
    // Three blocks: coefficient index, delta, two header samples, nibbles.
    let mut bytes = vec![];
    for k in 0..3u8 {
        bytes.push(k % 7);
        bytes.extend((40i16 + 30 * k as i16).to_le_bytes());
        bytes.extend((1200i16 - 500 * k as i16).to_le_bytes());
        bytes.extend((-800i16 + 300 * k as i16).to_le_bytes());
        bytes.extend((0..63).map(|i| (i * 29 + k as i32 * 7) as u8));
    }
    for (i, b) in bytes.iter().enumerate() {
        m.set_u8(DATA + 0x3000 + i as u32, *b);
    }
    for (i, v) in [0x40, 210, DATA + 0x3000, 0, 0, 0, 0, 0, 7]
        .into_iter()
        .enumerate()
    {
        m.set_u32(DATA + 0x200 + 4 * i as u32, v);
    }
    assert_eq!(method(&mut m, &r, voice, 21, &[DATA + 0x200, 0]), 0);
    assert_eq!(method(&mut m, &r, voice, 19, &[0, 0]), 0);
    sleep(&mut m, 60);
    let (left, right) = take_output(&output);
    let mut format = vec![0u8; 50];
    for (i, b) in format.iter_mut().enumerate() {
        *b = m.u8(f + i as u32);
    }
    let mut want = vec![];
    ddaudio::Format::parse(&format)
        .unwrap()
        .decode(&bytes, 0, 384, &mut want);
    // The first decoded sample (block 0's second header sample) is not 0.
    assert_contains(&left, &want);
    assert_contains(&right, &want);
    assert_eq!(method(&mut m, &r, e, 2, &[]), 0);
    sleep(&mut m, 15);
    assert!(threads::faults().is_empty(), "{:?}", threads::faults());
}

// A guest XAPO: QueryInterface (IID_IXAPO expected), LockForProcess
// (records the channel count, format tag and maximum frames), Process (in
// place: halves the buffer when enabled and the input is valid, copies the
// flag to the output).
fn xapo_query(m: &mut dyn Mem, c: &mut Cpu) {
    let (this, iid, out) = (m.u32(c.esp + 4), m.u32(c.esp + 8), m.u32(c.esp + 12));
    // IXAPO, 2.7 IID (game rdata 0x0142AF28)
    assert_eq!((m.u32(iid), m.u32(iid + 12)), (0xa90bc001, 0));
    m.set_u32(out, this);
    // QueryInterface adds a reference (the counter shared with fx_ref).
    m.set_u32(DATA + 0xa00, m.u32(DATA + 0xa00) + 1);
    c.eax = 0;
    c.esp += 16;
}
fn xapo_lock(m: &mut dyn Mem, c: &mut Cpu) {
    let input = m.u32(c.esp + 12);
    let format = m.u32(input);
    m.set_u32(DATA + 0xf00, m.u16(format + 2) as u32);
    m.set_u32(DATA + 0xf04, m.u16(format) as u32);
    m.set_u32(DATA + 0xf08, m.u32(input + 4));
    c.eax = 0;
    c.esp += 24;
}
fn xapo_halve(m: &mut dyn Mem, c: &mut Cpu) {
    let (input, output, enabled) = (m.u32(c.esp + 12), m.u32(c.esp + 20), m.u32(c.esp + 24));
    let (data, flags, frames) = (m.u32(input), m.u32(input + 4), m.u32(input + 8));
    if enabled != 0 && flags == 1 {
        for i in 0..frames * m.u32(DATA + 0xf00) {
            let x = f32::from_bits(m.u32(data + 4 * i)) * 0.5;
            m.set_u32(data + 4 * i, x.to_bits());
        }
    }
    m.set_u32(output + 4, flags);
    m.set_u32(output + 8, frames);
    m.set_u32(DATA + 0xf0c, m.u32(DATA + 0xf0c) + 1);
    c.esp += 28;
}

#[test]
fn xaudio_runs_guest_xapos_and_skips_unlifted_ones() {
    let (mut m, r) = setup();
    let output = audio::output();
    let e = engine(&mut m, &r);
    // Effect 0 cannot run: its LockForProcess is not bound to any code.
    // Effect 1 halves the mix.
    for (object, lock) in [(DATA + 0xc00, 0x3000u32), (DATA + 0xc80, 0x10c0)] {
        let vt = object + 0x40;
        m.set_u32(object, vt);
        for (slot, target) in [
            (0, 0x10b0),
            (1, 0x1080),
            (2, 0x1090),
            (8, lock),
            (9, 0x1010),
            (10, 0x10d0),
        ] {
            m.set_u32(vt + 4 * slot, target);
        }
    }
    // Both objects start with one reference (shared counter).
    m.set_u32(DATA + 0xa00, 2);
    let chain = DATA + 0xe00;
    m.set_u32(chain, 2);
    m.set_u32(chain + 4, chain + 8);
    for (i, object) in [DATA + 0xc00, DATA + 0xc80].into_iter().enumerate() {
        let d = chain + 8 + 12 * i as u32;
        m.set_u32(d, object);
        m.set_u32(d + 4, 1);
        m.set_u32(d + 8, 2);
    }
    assert_eq!(method(&mut m, &r, e, 10, &[DATA + 32, 0, 0, 0, 0, chain]), 0);
    let master = m.u32(DATA + 32);
    let voice = source(&mut m, &r, e, 0);
    let pcm: Vec<i16> = (0..480).map(|i| (i * 53 - 12000) as i16).collect();
    for (i, x) in pcm.iter().enumerate() {
        m.set_u16(DATA + 0x1000 + 2 * i as u32, *x as u16);
    }
    buffer(&mut m, 0x1234, 0x40);
    assert_eq!(method(&mut m, &r, voice, 21, &[DATA + 0x200, 0]), 0);
    assert_eq!(method(&mut m, &r, voice, 19, &[0, 0]), 0);
    sleep(&mut m, 60);
    let (left, right) = take_output(&output);
    let want: Vec<f32> = pcm.iter().map(|x| *x as f32 / 32768.0 * 0.5).collect();
    assert_contains(&left, &want);
    assert_contains(&right, &want);
    // Locked once with the mastering voice's float format: stereo,
    // WAVE_FORMAT_EXTENSIBLE, 480 frames per pass.
    assert_eq!(
        (m.u32(DATA + 0xf00), m.u32(DATA + 0xf04), m.u32(DATA + 0xf08)),
        (2, 0xfffe, 480)
    );
    assert!(m.u32(DATA + 0xf0c) > 3);
    method(&mut m, &r, voice, 18, &[]);
    method(&mut m, &r, master, 18, &[]);
    // Every reference taken (effect chain, QueryInterface) was released.
    assert_eq!(m.u32(DATA + 0xa00), 2);
    assert_eq!(method(&mut m, &r, e, 2, &[]), 0);
    sleep(&mut m, 15);
    assert!(threads::faults().is_empty(), "{:?}", threads::faults());
}
