use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Client;
use serde::de::DeserializeOwned;

use crate::config::AuthCredential;
use crate::types::ApiError;

pub struct ApiClient {
    client: Client,
    pub base_url: String,
    auth: Option<AuthCredential>,
}

impl ApiClient {
    pub fn new(base_url: &str, auth: Option<AuthCredential>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            auth,
        }
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth {
            Some(AuthCredential::ApiKey(key)) => req.header("X-API-Key", key),
            Some(AuthCredential::Bearer(token)) => {
                req.header("Authorization", format!("Bearer {}", token))
            }
            None => req,
        }
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let req = self.client.get(&url);
        let req = self.apply_auth(req);

        let response = req
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            self.handle_error(response).await
        }
    }

    pub async fn post<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let req = self.client.post(&url).json(body);
        let req = self.apply_auth(req);

        let response = req
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            self.handle_error(response).await
        }
    }

    pub async fn post_raw<B: serde::Serialize>(&self, path: &str, body: &B) -> Result<String> {
        let url = format!("{}{}", self.base_url, path);
        let req = self.client.post(&url).json(body);
        let req = self.apply_auth(req);

        let response = req
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.text().await.context("Failed to read response")
        } else {
            self.handle_error(response).await
        }
    }

    pub async fn post_form<T: DeserializeOwned>(
        &self,
        path: &str,
        form: &[(&str, &str)],
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let req = self.client.post(&url).form(form);

        let response = req
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            self.handle_error(response).await
        }
    }

    pub async fn patch<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let req = self.client.patch(&url).json(body);
        let req = self.apply_auth(req);

        let response = req
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            self.handle_error(response).await
        }
    }

    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let req = self.client.delete(&url);
        let req = self.apply_auth(req);

        let response = req
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            self.handle_error(response).await
        }
    }

    pub async fn delete_no_body(&self, path: &str) -> Result<()> {
        let url = format!("{}{}", self.base_url, path);
        let req = self.client.delete(&url);
        let req = self.apply_auth(req);

        let response = req
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            Ok(())
        } else {
            self.handle_error(response).await
        }
    }

    pub async fn stream_events(
        &self,
        job_id: &str,
    ) -> Result<impl futures::Stream<Item = Result<String>>> {
        let url = format!("{}/job/{}/events", self.base_url, job_id);
        let req = self.client.get(&url);
        let req = self.apply_auth(req);

        let response = req
            .send()
            .await
            .context("Failed to connect to API server")?;

        if !response.status().is_success() {
            let error: ApiError = response.json().await.context("Failed to parse error")?;
            anyhow::bail!("{}: {}", error.code, error.error)
        }

        Ok(futures::stream::unfold(
            response,
            |mut response| async move {
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        let text = String::from_utf8_lossy(&chunk).to_string();
                        Some((Ok(text), response))
                    }
                    Ok(None) => None,
                    Err(e) => Some((Err(anyhow::anyhow!("Stream error: {}", e)), response)),
                }
            },
        ))
    }

    async fn handle_error<T>(&self, response: reqwest::Response) -> Result<T> {
        let status = response.status();
        match response.json::<ApiError>().await {
            Ok(error) => anyhow::bail!("{}: {}", error.code, error.error),
            Err(_) => anyhow::bail!("Request failed with status {}", status),
        }
    }
}
