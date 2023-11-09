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
use reqwest::header::{HeaderValue, USER_AGENT};
use reqwest::multipart::Form;
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use url::Url;

use crate::cli::ApiVersion;
use crate::prompt;

pub struct Request {
    host: String,
    ver: ApiVersion,
    creds: (Option<String>, Option<String>),
    inner: reqwest::Request,
    multipart: Option<Form>,
}

impl Request {
    pub fn new(
        host: String,
        ver: ApiVersion,
        creds: (Option<String>, Option<String>),
        user_agent: &str,
    ) -> Result<Self> {
        let url = url_from_host(&host, ver.scheme())?;
        let mut inner = reqwest::Request::new(Method::GET, url);
        inner
            .headers_mut()
            .insert(USER_AGENT, HeaderValue::from_str(user_agent)?);

        Ok(Self {
            host,
            ver,
            creds,
            inner,
            multipart: None,
        })
    }

    pub fn to_post(&self) -> Result<Self> {
        let url = url_from_host(&self.host, self.ver.scheme())?;
        let inner = reqwest::Request::new(Method::POST, url);

        Ok(Self {
            host: self.host.clone(),
            ver: self.ver,
            creds: self.creds.clone(),
            inner,
            multipart: None,
        })
    }

    pub fn set_multipart(&mut self, form: Form) {
        self.multipart = Some(form);
    }

    pub async fn send(mut self, client: Client) -> Result<Response> {
        let mut authenticated = cfg!(not(feature = "localhost"));

        let resp = loop {
            let mut builder =
                RequestBuilder::from_parts(client.clone(), self.inner.try_clone().unwrap());

            if authenticated {
                let token = self.get_bearer_token(&client).await?;
                builder = builder.bearer_auth(token);
            }

            if let Some(form) = self.multipart.take() {
                builder = builder.multipart(form);
            }

            let resp = builder.send().await?;
            if resp.status() == StatusCode::UNAUTHORIZED {
                delete_cached_token();
                authenticated = true;
            } else {
                break resp;
            }
        };

        Ok(resp)
    }

    async fn get_bearer_token(&mut self, client: &Client) -> Result<String> {
        // If either credentials are supplied, use them
        if self.creds.0.is_some() || self.creds.1.is_some() {
            return request_token(&self.host, self.ver, &self.creds, client).await;
        }

        // Else, try retrieving cached token from a file
        if let Some(token) = get_cached_token() {
            return Ok(token);
        }

        // If it doesn't exist, ask on an interactive prompt
        request_token(&self.host, self.ver, &self.creds, client).await
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
            creds: self.creds.clone(),
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

fn get_cached_token() -> Option<String> {
    let path = get_cache_file_location();
    let file = std::fs::read_to_string(path);

    file.ok()
}

fn delete_cached_token() {
    let _ = std::fs::remove_file(get_cache_file_location());
}

fn get_cache_file_location() -> PathBuf {
    let default = PathBuf::from(".");
    let mut path = dirs::cache_dir().unwrap_or(default);

    path.push("tpi_token");

    path
}

async fn request_token(
    host: &str,
    ver: ApiVersion,
    creds: &(Option<String>, Option<String>),
    client: &Client,
) -> Result<String> {
    let mut auth_url = url_from_host(host, ver.scheme())?;

    auth_url
        .path_segments_mut()
        .expect("URL cannot be a base")
        .push("authenticate");

    // Save token to a file only if credentials weren't supplied from the command line
    let save_token = creds.0.is_none() && creds.1.is_none();

    let (username, password) = match creds.clone() {
        (Some(username), Some(password)) => (username, password),
        (Some(username), None) => {
            let password = prompt::password("Password")?;
            (username, password)
        }
        (None, Some(password)) => {
            let username = prompt::simple("User")?;
            (username, password)
        }
        (None, None) => {
            let username = prompt::simple("User")?;
            let password = prompt::password("Password")?;
            (username, password)
        }
    };

    let body = serde_json::json!({
        "username": username,
        "password": password
    });

    let resp = client.post(auth_url).json(&body).send().await?;

    match resp.status() {
        StatusCode::OK => {
            let json = resp.json::<serde_json::Value>().await?;
            let token = get_param(&json, "id");

            if save_token {
                if let Err(e) = cache_token(&token) {
                    let path = get_cache_file_location();
                    println!("Warning: failed to write to cache file {:?}: {}", path, e);
                }
            }

            Ok(token)
        }
        StatusCode::FORBIDDEN => bail!(
            "{}",
            resp.text()
                .await
                .unwrap_or("could not authenticate".to_string())
        ),
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
