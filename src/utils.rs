pub fn usize_to_isize(value: usize) -> isize {
    if value > isize::MAX as usize {
        (value - isize::MAX as usize - 1) as isize
    } else {
        value as isize
    }
}
pub fn isize_to_usize(value: isize) -> usize {
    if value < 0 {
        (value + isize::MAX + 1) as usize
    } else {
        value as usize
    }
}
