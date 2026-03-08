use rand::RngExt;

pub const LOCAL_ID_LEN: usize = 16;
pub const LOCAL_ID_ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstuvwxyz";

pub type LocalId = String;

pub fn generate_local_id() -> LocalId {
    let mut rng = rand::rng();
    (0..LOCAL_ID_LEN)
        .map(|_| {
            let idx = rng.random_range(0..LOCAL_ID_ALPHABET.len());
            LOCAL_ID_ALPHABET[idx] as char
        })
        .collect()
}

pub fn normalize_local_id(raw: &str) -> Option<LocalId> {
    let trimmed = raw.trim();
    if is_local_id(trimmed) {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

pub fn is_local_id(value: &str) -> bool {
    value.len() == LOCAL_ID_LEN
        && value
            .as_bytes()
            .iter()
            .all(|byte| LOCAL_ID_ALPHABET.contains(byte))
}

#[cfg(test)]
pub fn test_local_id(seed: &str) -> LocalId {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut id = String::with_capacity(LOCAL_ID_LEN);
    let mut counter = 0_u64;
    while id.len() < LOCAL_ID_LEN {
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        counter.hash(&mut hasher);
        let mut value = hasher.finish();
        for _ in 0..8 {
            let idx = (value % LOCAL_ID_ALPHABET.len() as u64) as usize;
            id.push(LOCAL_ID_ALPHABET[idx] as char);
            if id.len() == LOCAL_ID_LEN {
                break;
            }
            value /= LOCAL_ID_ALPHABET.len() as u64;
        }
        counter += 1;
    }
    id
}

#[cfg(test)]
mod tests {
    use super::{LOCAL_ID_LEN, generate_local_id, is_local_id, normalize_local_id, test_local_id};

    #[test]
    fn generates_valid_ids() {
        let id = generate_local_id();
        assert_eq!(id.len(), LOCAL_ID_LEN);
        assert!(is_local_id(&id));
    }

    #[test]
    fn normalizes_trimmed_ids() {
        let id = test_local_id("abc123");
        assert_eq!(normalize_local_id(&format!("  {id}  ")), Some(id));
    }
}
