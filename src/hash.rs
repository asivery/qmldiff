pub fn hash_h(string: &str, seed: u64) -> u64 {
    let mut hash = seed;

    for char in string.chars() {
        let char = char as u8 as u64;
        hash = ((hash << 5).wrapping_add(hash)).wrapping_add(char);
    }

    hash
}

pub fn hash(string: &str) -> u64 {
    hash_h(string, 5481)
}
