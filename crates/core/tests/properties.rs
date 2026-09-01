mod tests {
    use work_time_core::{
        DomainError, EntrySource, ProjectId, TimeEntry, validate_color, validate_name,
    };

    #[test]
    fn blank_names_are_rejected() {
        assert_eq!(validate_name("   "), Err(DomainError::EmptyName));
        assert!(validate_name("Design").is_ok());
    }

    #[test]
    fn colors_must_be_seven_char_hex() {
        assert!(validate_color("#3584e4").is_ok());
        assert_eq!(validate_color("3584e4"), Err(DomainError::InvalidColor));
        assert_eq!(validate_color("#35g4e4"), Err(DomainError::InvalidColor));
    }

    #[test]
    fn duration_never_goes_negative() {
        let entry = TimeEntry {
            id: None,
            project_id: ProjectId(1),
            task_id: None,
            note: String::new(),
            start_ms: 200,
            end_ms: 100,
            source: EntrySource::Manual,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        assert_eq!(entry.duration_ms(), 0);
        assert!(entry.validate().is_err());
    }
}
