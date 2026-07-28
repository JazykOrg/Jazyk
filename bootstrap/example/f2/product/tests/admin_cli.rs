// tests/admin_cli.rs
use std::fs;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc, Duration};

// Assuming src/admin_cli.rs is available in the test environment structure.
// We need to import the necessary components from the product module.
use product::admin_cli::{AdminCli, Config, Order};

#[test]
fn test_req_admin_1_refusal_on_missing_config() {
    // req:admin-1 [req_admin_1_167f2d57]: The Admin CLI shall refuse to start when the configuration file is missing.
    let non_existent_path = PathBuf::from("non_existent_config.toml");
    
    // Test case where the config file is missing.
    let result = AdminCli::new(&non_existent_path);

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Configuration file is missing")); 
}

#[test]
fn test_req_admin_1_success_on_valid_config() {
    // req:admin-1 [req_admin_1_167f2d57]: The Admin CLI shall refuse to start when the configuration file is missing.
    let temp_config_path = PathBuf::from("temp_admin_config.toml");

    // Create a dummy config file for testing success case
    if let Ok(mut file) = std::fs::File::create(&temp_config_path) {
        use std::io::Write;
        if write!(file, "database=sqlite").is_err() {
            panic!("Failed to write temp config file");
        }
    } else {
        panic!("Could not create temporary config file");
    }

    let result = AdminCli::new(&temp_config_path);
    assert!(result.is_ok()); 

    // Cleanup
    std::fs::remove_file(&temp_config_path).unwrap();
}


#[test]
fn test_req_admin_2_report_includes_all_orders() {
    // req:admin-2 [req_admin_2_4b49d75a]: When an operator requests a report, the Admin CLI shall include every Order from the selected period.
    let temp_config_path = PathBuf::from("temp_admin_config_for_reporting.toml");

    // Setup: Create a valid config file
    if let Ok(mut file) = std::fs::File::create(&temp_config_path) {
        use std::io::Write;
        if write!(file, "database=sqlite").is_err() {
            panic!("Failed to write temp config file");
        }
    } else {
        panic!("Could not create temporary config file");
    }

    // Initialize CLI
    let cli = AdminCli::new(&temp_config_path).expect("CLI initialization failed"); 
    
    // Define a reporting period (e.g., last 10 days)
    let end_date = Utc::now();
    let start_date = end_date - Duration::days(15);

    // Execute the function under test
    let report = cli.generate_report(start_date, end_date);

    // Check if the number of orders returned matches the expected mock count (3)
    assert_eq!(report.len(), 3); 

    // Cleanup
    std::fs::remove_file(&temp_config_path).unwrap();
}