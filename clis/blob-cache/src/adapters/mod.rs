//! One adapter per technology behind `BlobStore` and `BlobAdmin`.

pub mod dir;
pub mod http;
pub mod oci_cli;
pub mod par;
pub mod sigv4;
pub mod tar;

pub use dir::DirStore;
pub use http::{Auth, HttpStore, S3Location, Side};
pub use oci_cli::{OciCli, OciTenancy};
pub use par::OciParStore;
pub use tar::TarGz;
