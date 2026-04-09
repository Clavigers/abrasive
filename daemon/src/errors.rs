use crate::constants::REQUIRED_ORG;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("github /user call failed: {0}")]
    UserCall(#[source] ureq::Error),

    #[error("could not parse /user response: {0}")]
    UserResponseParse(#[source] std::io::Error),

    #[error("no 'login' field in /user response")]
    NoLoginField,

    #[error("user '{login}' is not a member of {}", REQUIRED_ORG)]
    NotMember { login: String },

    #[error("github membership check failed: {0}")]
    MembershipCheck(#[source] ureq::Error),
}
