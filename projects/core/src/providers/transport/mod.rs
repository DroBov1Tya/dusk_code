//! How a provider reaches its server: the HTTP client every protocol shares, and the
//! decoder of the event streams they answer with.

pub mod http;
pub mod sse;
