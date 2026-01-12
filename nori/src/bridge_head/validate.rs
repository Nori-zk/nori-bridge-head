use crate::utils::panic_more;

/// Terminates the program if any required variable is missing
pub fn validate_env(env_vars: &[&str]) {
    for var_name in env_vars {
        if std::env::var(var_name).is_err() {
            panic_more(&format!("Critical boot error: Missing {} environment variable", var_name));
        }
    }
}
