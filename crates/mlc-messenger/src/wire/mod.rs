//! Wire-boundary data types: the shapes that cross this crate's async
//! boundary as plain data (see the crate doc's "Contract" section). This
//! module never performs an HTTP call itself — it only builds the request
//! description and parses the response description a shell hands back.

mod requests;

pub use requests::{HttpMethod, HttpResponseDescriptor, OutgoingRequest, OutgoingRequestKind};
