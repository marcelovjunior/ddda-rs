//! Host upload preparation. Preserve source bytes and row order while avoiding
//! a second allocation/copy when the owned upload is already GPU-ready.
use std::borrow::Cow;

/// Direct-format images already in tight row/slice order can be borrowed for
/// Queue::write_texture (which copies them before returning). Padded rows keep
/// the previous compaction behavior, including ignoring final-row padding.
pub(crate) fn direct_image(bytes: &[u8], row: u32, rows: u32, depth: u32, pitch: u32) -> Cow<'_, [u8]> {
    let len = row as usize * rows as usize * depth as usize;
    if row == pitch {
        return Cow::Borrowed(&bytes[..len]);
    }
    // Preserve the existing allocator/growth path for padded rows. A single
    // exact reservation was slower in the host allocator microbenchmark.
    let mut packed = Vec::new();
    for z in 0..depth {
        let slice = &bytes[(z * pitch * rows) as usize..];
        for y in 0..rows {
            let start = (y * pitch) as usize;
            packed.extend_from_slice(&slice[start..start + row as usize]);
        }
    }
    Cow::Owned(packed)
}

/// Buffer uploads are owned already. Extend that allocation by at most three
/// bytes (four for an empty buffer) instead of cloning the entire payload.
pub(crate) fn pad_buffer(bytes: &mut Vec<u8>) {
    bytes.resize(bytes.len().next_multiple_of(4).max(4), 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn previous_image(bytes: &[u8], row: u32, rows: u32, depth: u32, pitch: u32) -> Vec<u8> {
        let mut packed = Vec::new();
        for z in 0..depth {
            let slice = &bytes[(z * pitch * rows) as usize..];
            for y in 0..rows {
                let start = (y * pitch) as usize;
                packed.extend_from_slice(&slice[start..start + row as usize]);
            }
        }
        packed
    }

    #[test]
    fn direct_rows_match_previous_packing_for_mips_blocks_and_volumes() {
        // Rows are block rows for BC formats, texel rows otherwise. Cover
        // short final-row padding and multiple volume slices as well.
        for row in [1, 3, 4, 8, 16, 48, 256] {
            for rows in [1, 2, 7] {
                for depth in [1, 2, 3, 4, 8] {
                    for padding in [0, 1, 4, 32] {
                        let pitch = row + padding;
                        let len = (pitch * (rows * depth - 1) + row) as usize;
                        let bytes: Vec<_> = (0..len).map(|i| (i * 37) as u8).collect();
                        let actual = direct_image(&bytes, row, rows, depth, pitch);
                        assert_eq!(&*actual, previous_image(&bytes, row, rows, depth, pitch));
                        if padding == 0 {
                            assert!(matches!(actual, Cow::Borrowed(_)));
                            assert_eq!(actual.as_ptr(), bytes.as_ptr());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn owned_buffer_padding_preserves_payload_and_allocation_when_possible() {
        for len in 0usize..=257 {
            let mut bytes = Vec::with_capacity(264);
            bytes.extend((0..len).map(|i| (i * 19) as u8));
            let pointer = bytes.as_ptr();
            let mut previous = bytes.clone();
            previous.resize(len.next_multiple_of(4).max(4), 0);
            pad_buffer(&mut bytes);
            assert_eq!(bytes, previous);
            assert_eq!(bytes.as_ptr(), pointer);
        }
    }
}
