// Copyright 2023 Turing Machines
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Wrapper for `reqwest::Request` that asks for authentication if needed.

use std::io::Write;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;

use anyhow::{bail, Result};
use reqwest::multipart::Form;
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use url::Url;

use crate::cli::ApiVersion;
use crate::prompt;

pub struct Request {
    pub host: String,
    pub ver: ApiVersion,
    inner: reqwest::Request,
    multipart: Option<Form>,
}

impl Request {
    pub fn new(host: String, ver: ApiVersion) -> Result<Self> {
        Self::construct_request(host, ver, Method::GET)
    }

    pub fn new_post(host: String, ver: ApiVersion) -> Result<Self> {
        Self::construct_request(host, ver, Method::POST)
    }

    fn construct_request(host: String, ver: ApiVersion, method: reqwest::Method) -> Result<Self> {
        let url = url_from_host(&host, ver.scheme())?;
        let inner = reqwest::Request::new(method, url);

        Ok(Self {
            host,
            ver,
            inner,
            multipart: None,
        })
    }

    pub fn set_multipart(&mut self, form: Form) {
        self.multipart = Some(form);
    }

    pub async fn send(mut self, client: Client) -> Result<Response> {
        let token = self.auth_token(&client, true).await?;
        let mut builder = RequestBuilder::from_parts(client, self.inner).bearer_auth(token);

        if let Some(form) = self.multipart {
            builder = builder.multipart(form);
        }

        let resp = builder.send().await?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            bail!("{}", resp.status());
        }
        Ok(resp)
    }

    async fn auth_token(&mut self, client: &Client, use_cache: bool) -> Result<String> {
        if use_cache {
            get_bearer_token(&self.host, self.ver, client).await
        } else {
            request_token(&self.host, self.ver, client).await
        }
    }

    pub fn url(&self) -> &Url {
        self.inner.url()
    }

    pub fn url_mut(&mut self) -> &mut Url {
        self.inner.url_mut()
    }

    pub fn clone(&self) -> Self {
        let inner = self
            .inner
            .try_clone()
            .expect("request cannot be cloned: body is a stream");

        Self {
            host: self.host.clone(),
            ver: self.ver,
            inner,
            multipart: None,
        }
    }
}

impl Deref for Request {
    type Target = reqwest::Request;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Request {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

fn url_from_host(host: &str, scheme: &str) -> Result<Url> {
    let mut url = Url::parse(&format!("{}://{}", scheme, host))?;
    url.set_path("api/bmc");
    Ok(url)
}

async fn get_bearer_token(host: &str, ver: ApiVersion, client: &Client) -> Result<String> {
    if let Some(token) = get_cached_token() {
        return Ok(token);
    }

    request_token(host, ver, client).await
}

fn get_cached_token() -> Option<String> {
    let path = get_cache_file_location();
    let file = std::fs::read_to_string(path);

    file.ok()
}

fn get_cache_file_location() -> PathBuf {
    let default = PathBuf::from(".");
    let mut path = dirs::cache_dir().unwrap_or(default);

    path.push("tpi_token");

    path
}

async fn request_token(host: &str, ver: ApiVersion, client: &Client) -> Result<String> {
    let mut auth_url = url_from_host(host, ver.scheme())?;

    auth_url
        .path_segments_mut()
        .expect("URL cannot be a base")
        .push("authenticate");

    let username = prompt::simple("User")?;
    let password = prompt::password("Password")?;

    let body = serde_json::json!({
        "username": username,
        "password": password
    });

    let resp = client.post(auth_url).json(&body).send().await?;

    match resp.status() {
        StatusCode::OK => {
            let json = resp.json::<serde_json::Value>().await?;
            let token = get_param(&json, "id");

            if let Err(e) = cache_token(&token) {
                let path = get_cache_file_location();
                println!("Warning: failed to write to cache file {:?}: {}", path, e);
            }

            Ok(token)
        }
        StatusCode::FORBIDDEN => bail!("Incorrect credentials"),
        x => bail!("Unexpected status code {x}"),
    }
}

fn get_param(results: &serde_json::Value, key: &str) -> String {
    results
        .get(key)
        .unwrap_or_else(|| panic!("API error: Expected `{key}` attribute"))
        .as_str()
        .unwrap_or_else(|| panic!("API error: `{key}` value is not a string"))
        .to_owned()
}

fn cache_token(token: &str) -> Result<()> {
    let path = get_cache_file_location();

    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?
        .write_all(token.as_bytes())?;

    Ok(())
}
