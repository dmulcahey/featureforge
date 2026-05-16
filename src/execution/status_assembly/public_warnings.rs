pub(crate) fn public_status_warning_code(warning_code: &str) -> String {
    warning_code.replace("receipt", "projection")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_shaped_warning_terms_are_projected_as_diagnostics() {
        assert_eq!(
            public_status_warning_code("serial_unit_review_receipt_missing_diagnostic_only"),
            "serial_unit_review_projection_missing_diagnostic_only"
        );
        assert_eq!(
            public_status_warning_code("plain_unit_review_receipts_diagnostic_only"),
            "plain_unit_review_projections_diagnostic_only"
        );
    }
}
