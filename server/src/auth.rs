//! The current user. There is one: every request acts as the default user,
//! whether sign-in is off (the loopback listener trusts its caller) or on
//! (`modules::auth` checks who it is first; ADR 0042). Libraries per user
//! come later.

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
