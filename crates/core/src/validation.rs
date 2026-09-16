use crate::DomainError;

pub fn validate_name(name: &str) -> Result<(), DomainError> {
    if name.trim().is_empty() {
        Err(DomainError::EmptyName)
    } else {
        Ok(())
    }
}

pub fn validate_color(color: &str) -> Result<(), DomainError> {
    let valid = color.len() == 7
        && color.starts_with('#')
        && color.bytes().skip(1).all(|byte| byte.is_ascii_hexdigit());
    if valid {
        Ok(())
    } else {
        Err(DomainError::InvalidColor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_require_a_non_whitespace_character() {
        for name in ["", " ", "\n\t", "\u{2003}"] {
            assert_eq!(validate_name(name), Err(DomainError::EmptyName));
        }
        for name in ["a", " Design ", "時間", "🦀"] {
            assert_eq!(validate_name(name), Ok(()));
        }
    }
    #[test]
    fn colors_require_exactly_six_ascii_hex_digits() {
        for color in ["#000000", "#ffffff", "#Ab09Ef"] {
            assert_eq!(validate_color(color), Ok(()));
        }
        for color in [
            "", "ffffff", "#fff", "#1234567", "#gg0000", " #000000", "#é0000",
        ] {
            assert_eq!(validate_color(color), Err(DomainError::InvalidColor));
        }
    }
}
