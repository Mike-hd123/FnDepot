//! Concrete, non-identity codec strategies.

pub mod messages_to_responses;
pub mod responses_to_messages;

pub use messages_to_responses::MESSAGES_TO_RESPONSES_V2;
pub use responses_to_messages::RESPONSES_TO_MESSAGES_V2;
