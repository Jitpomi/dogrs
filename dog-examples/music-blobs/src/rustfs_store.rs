//! RustFS uses the tested S3-compatible backend, including native multipart and streaming reads.
pub use dog_blob::S3CompatibleStore as RustFSStore;
