//! 协议标识符：UUID 或 26 位 Crockford base32（ULID 风格）。

const UUID_LEN: usize = 36;
const CROCKFORD_LEN: usize = 26;
const CROCKFORD_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

pub fn is_valid_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == UUID_LEN
        && b.iter().enumerate().all(|(i, &c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

pub fn is_valid_identifier(s: &str) -> bool {
    if is_valid_uuid(s) {
        return true;
    }
    let b = s.as_bytes();
    b.len() == CROCKFORD_LEN && b.iter().all(|c| CROCKFORD_ALPHABET.contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_forms() {
        assert!(is_valid_uuid("5f8c2b1e-9a3d-4c2e-b1f0-6d7e8a9b0c1d"));
        assert!(is_valid_uuid("5F8C2B1E-9A3D-4C2E-B1F0-6D7E8A9B0C1D"));
        assert!(!is_valid_uuid("5f8c2b1e9a3d4c2eb1f06d7e8a9b0c1d")); // 无连字符
        assert!(!is_valid_uuid("5f8c2b1e-9a3d-4c2e-b1f0-6d7e8a9b0c1g"));
        assert!(!is_valid_uuid(""));
    }

    #[test]
    fn crockford_forms() {
        assert!(is_valid_identifier("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert!(!is_valid_identifier("01ARZ3NDEKTSV4RRFFQ69G5FAI")); // I 非法
        assert!(!is_valid_identifier("01ARZ3NDEKTSV4RRFFQ69G5FA")); // 25 位
        assert!(!is_valid_identifier("01arz3ndektsv4rrffq69g5fav")); // 必须大写
        assert!(is_valid_identifier("5f8c2b1e-9a3d-4c2e-b1f0-6d7e8a9b0c1d"));
    }
}
