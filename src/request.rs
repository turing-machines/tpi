//! Wrapper for `reqwest::Request` that asks for authentication if needed.

use anyhow::{Context, Result};
use reqwest::{Client, Method, Response};
use url::Url;

use crate::cli::ApiVersion;

pub struct Request {
    inner: reqwest::Request,
}

impl Request {
    pub fn new(host: String, ver: ApiVersion) -> Result<Self> {
        Self::construct_request(host, ver, Method::GET)
    }

    pub fn new_post(host: String, ver: ApiVersion) -> Result<Self> {
        Self::construct_request(host, ver, Method::POST)
    }

    fn construct_request(host: String, ver: ApiVersion, method: reqwest::Method) -> Result<Self> {
        let url = url_from_host(host, ver.scheme())?;
        let inner = reqwest::Request::new(method, url);

        Ok(Self { inner })
    }

    pub async fn send(self, client: &Client) -> Result<Response> {
        client
            .execute(self.inner)
            .await
            .context("HTTP request error")
    }

    pub fn url(&self) -> &Url {
        self.inner.url()
    }

    pub fn url_mut(&mut self) -> &mut Url {
        self.inner.url_mut()
    }

    pub fn as_mut(&mut self) -> &mut reqwest::Request {
        &mut self.inner
    }

    pub fn clone(&self) -> Self {
        let inner = self
            .inner
            .try_clone()
            .expect("request cannot be cloned: body is a stream");

        Self { inner }
    }
}

fn url_from_host(host: String, scheme: &str) -> Result<Url> {
    let mut url = Url::parse(&format!("{}://{}", scheme, host))?;
    url.set_path("api/bmc");
    Ok(url)
}
