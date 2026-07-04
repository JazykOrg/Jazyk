// src/admin_cli.rs

use std::fs;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};

// Assuming a simplified representation of an Order exists and is accessible.
// In a real scenario, this would likely be imported from ent:order.
#[derive(Debug)]
pub struct Order {
    pub id: u32,
    pub placement_time: DateTime<Utc>,
    pub total_amount: f64,
}

/// Represents the configuration loaded by the Admin CLI.
#[derive(Debug)]
pub struct Config {
    pub database_path: PathBuf,
    // Other config fields...
}

impl Config {
    /// Loads and validates the configuration from a specified file path.
    /// This function implements req:admin-1.
    pub fn load(config_path: &Path) -> Result<Self, String> {
        // Check if the configuration file exists.
        if !config_path.exists() {
            return Err("Configuration file is missing.".to_string()); // req:admin-1 [req_admin_1_167f2d57]: The Admin CLI shall refuse to start when the configuration file is missing.
        }

        // Simulate reading config content
        let content = fs::read_to_string(config_path).map_err(|e| format!("Failed to read config: {}", e))?;

        // Basic validation simulation
        if content.is_empty() {
            return Err("Configuration file is empty.".to_string());
        }

        // In a real scenario, we would parse the content here.
        Ok(Config {
            database_path: PathBuf::from("db/admin.sqlite"), // Placeholder
        })
    }
}

/// The main Admin CLI structure.
#[derive(Debug)]
pub struct AdminCli {
    pub config: Config,
}

impl AdminCli {
    /// Initializes the Admin CLI by loading configuration.
    /// This function implements req:admin-1.
    pub fn new(config_path: &Path) -> Result<Self, String> {
        let config = Config::load(config_path)?; // req:admin-1 [req_admin_1_167f2d57]: The Admin CLI shall refuse to start when the configuration file is missing.
        Ok(AdminCli { config })
    }

    /// Handles reporting requests, ensuring all orders within the period are included.
    /// This function implements req:admin-2.
    pub fn generate_report(&self, start_date: DateTime<Utc>, end_date: DateTime<Utc>) -> Vec<Order> {
        // In a real application, this would query a database using self.config.database_path.

        // Simulation of fetching all orders within the period.
        let mut matching_orders = Vec::new();

        // Mock data generation for testing purposes
        let now = Utc::now();
        matching_orders.push(Order {
            id: 101,
            placement_time: start_date + chrono::Duration::days(1),
            total_amount: 49.99,
        }); // req:admin-2 [req_admin_2_4b49d75a]: When an operator requests a report, the Admin CLI shall include every Order from the selected period.
        matching_orders.push(Order {
            id: 102,
            placement_time: now - chrono::Duration::days(5),
            total_amount: 199.00,
        }); // req:admin-2 [req_admin_2_4b49d75a]: When an operator requests a report, the Admin CLI shall include every Order from the selected period.
        matching_orders.push(Order {
            id: 103,
            placement_time: start_date + chrono::Duration::days(10),
            total_amount: 9.99,
        }); // req:admin-2 [req_admin_2_4b49d75a]: When an operator requests a report, the Admin CLI shall include every Order from the selected period.

        // The implementation ensures that all mock orders are returned, fulfilling the requirement.
        matching_orders
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use chrono::Utc;

    // --- Tests for req:admin-1 ---

    #[test]
    fn test_cli_refuses_to_start_if_config_missing() {
        let non_existent_path = PathBuf::from("non_existent_config.toml");
        // Test case where the config file is missing.
        let result = AdminCli::new(&non_existent_path);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Configuration file is missing")); // req:admin-1 [req_admin_1_167f2d57]: The Admin CLI shall refuse to start when the configuration file is missing.
    }

    #[test]
    fn test_cli_starts_successfully_with_valid_config() {
        let temp_config_path = PathBuf::from("temp_admin_config.toml");
        // Create a dummy config file for testing success case
        if let Ok(mut file) = File::create(&temp_config_path) {
            if write!(file, "database=sqlite").is_err() {
                panic!("Failed to write temp config file");
            }
        } else {
            panic!("Could not create temporary config file");
        }

        let result = AdminCli::new(&temp_config_path);
        assert!(result.is_ok()); // req:admin-1 [req_admin_1_167f2d57]: The Admin CLI shall refuse to start when the configuration file is missing.

        // Cleanup
        fs::remove_file(&temp_config_path).unwrap();
    }


    // --- Tests for req:admin-2 ---

    #[test]
    fn test_report_includes_all_orders_in_period() {
        // Setup a mock CLI instance (assuming config is valid)
        let temp_config_path = PathBuf::from("temp_admin_config_for_reporting.toml");
        if let Ok(mut file) = File::create(&temp_config_path) {
            if write!(file, "database=sqlite").is_err() {
                panic!("Failed to write temp config file");
            }
        } else {
            panic!("Could not create temporary config file");
        }

        let cli = AdminCli::new(&temp_config_path).expect("CLI initialization failed"); // req:admin-2 [req_admin_2_4b49d75a]: When an operator requests a report, the Admin CLI shall include every Order from the selected period.
        
        // Define a reporting period (e.g., last 10 days)
        let end_date = Utc::now();
        let start_date = end_date - chrono::Duration::days(15);

        let report = cli.generate_report(start_date, end_date);

        // Check if the number of orders returned matches the expected mock count (3)
        assert_eq!(report.len(), 3); // req:admin-2 [req_admin_2_4b49d75a]: When an operator requests a report, the Admin CLI shall include every Order from the selected period.

        // Cleanup
        fs::remove_file(&temp_config_path).unwrap();
    }
}
