//! The current user. Authentication does not exist yet, so every request is
//! the single default user (server/internal/auth).

pub const DEFAULT_USER_ID: &str = "00000000-0000-0000-0000-000000000001";

#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub id: &'static str,
    pub username: &'static str,
    pub display_name: &'static str,
}

pub fn current_user() -> User {
    User {
        id: DEFAULT_USER_ID,
        username: "admin",
        display_name: "Admin",
    }
}
