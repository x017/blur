//! UUID generation utilities (v4 random and v7 time-ordered).

use uuid::Uuid;

/// Generate a random UUID v4 string.
pub fn uuid_v4() -> String {
    Uuid::new_v4().to_string()
}

/// Generate a time-ordered UUID v7 string.
pub fn uuid_v7() -> String {
    Uuid::now_v7().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_valid_uuids() {
        let v4 = Uuid::parse_str(&uuid_v4()).unwrap();
        assert_eq!(v4.get_version_num(), 4);
        let v7 = Uuid::parse_str(&uuid_v7()).unwrap();
        assert_eq!(v7.get_version_num(), 7);
    }
}
