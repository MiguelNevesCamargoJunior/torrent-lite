use torrent_lite::{
    magnet::Magnet,
    network_input::{sanitize_magnet, sanitize_torrent},
    torrent_file::inspect_torrent,
};

fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn mutate(seed: &[u8], state: &mut u64) -> Vec<u8> {
    let mut value = seed.to_vec();
    for _ in 0..1 + next(state) % 4 {
        let index = (next(state) as usize) % (value.len() + 1);
        match next(state) % 5 {
            0 if index < value.len() => value[index] = next(state) as u8,
            1 if value.len() < 512 => value.insert(index, next(state) as u8),
            2 if index < value.len() => {
                value.remove(index);
            }
            3 => value.truncate(index),
            _ if index < value.len() => value[index] ^= 1 << (next(state) % 8),
            _ => {}
        }
    }
    value
}

fn torrent_seed() -> Vec<u8> {
    let mut bytes = b"d4:infod6:lengthi4e4:name8:file.txt12:piece lengthi4e6:pieces20:".to_vec();
    bytes.extend([0_u8; 20]);
    bytes.extend(b"ee");
    bytes
}

#[test]
fn mutated_torrent_inputs_do_not_panic_or_change_the_hash_when_sanitized() {
    let seeds = [torrent_seed(), b"d4:infoi9223372036854775808ee".to_vec()];
    let mut state = 0x9e37_79b9_7f4a_7c15;
    for index in 0..2_000 {
        let input = mutate(&seeds[index % seeds.len()], &mut state);
        let checked = std::panic::catch_unwind(|| {
            let original = inspect_torrent(&input);
            if let Ok(sanitized) = sanitize_torrent(&input) {
                let original = original.expect("sanitizer accepted uninspectable input");
                let output =
                    inspect_torrent(&sanitized.bytes).expect("sanitizer emitted invalid metainfo");
                assert_eq!(original.info_hash, output.info_hash);
                assert_eq!(original.files, output.files);
            }
        });
        assert!(checked.is_ok(), "torrent mutation {index} panicked");
    }
}

#[test]
fn mutated_magnet_inputs_do_not_panic_or_change_the_hash_when_sanitized() {
    let seed = b"magnet:?xt=urn:btih:fc13cb96cc1f1dd111edc8c97537d90a7a3feba3&tr=https%3A%2F%2Fexample.org%2Fannounce";
    let mut state = 0x517c_c1b7_2722_0a95;
    for index in 0..2_000 {
        let bytes = mutate(seed, &mut state);
        let input = String::from_utf8_lossy(&bytes);
        let checked = std::panic::catch_unwind(|| {
            let original = Magnet::parse(&input);
            if let Ok(sanitized) = sanitize_magnet(&input) {
                let original = original.expect("sanitizer accepted invalid magnet");
                let output =
                    Magnet::parse(&sanitized.uri).expect("sanitizer emitted invalid magnet");
                assert_eq!(original.info_hash(), output.info_hash());
            }
        });
        assert!(checked.is_ok(), "magnet mutation {index} panicked");
    }
}
