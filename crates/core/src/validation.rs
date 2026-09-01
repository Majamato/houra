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
