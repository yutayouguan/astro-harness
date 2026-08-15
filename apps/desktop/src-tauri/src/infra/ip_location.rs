use std::time::Duration;

use serde::{Deserialize, Serialize};

const IP_LOCATION_URL: &str = "https://ipwho.is/";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpLocationDto {
    pub city: String,
    pub region: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IpWhoResponse {
    success: bool,
    city: Option<String>,
    region: Option<String>,
    country: Option<String>,
}

fn non_blank(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

fn parse_ipwho_response(body: &str) -> Result<IpLocationDto, String> {
    let response: IpWhoResponse =
        serde_json::from_str(body).map_err(|_| "Invalid IP location response".to_owned())?;
    if !response.success {
        return Err("IP location service rejected the request".to_owned());
    }
    let city = non_blank(response.city)
        .ok_or_else(|| "IP location response did not include a city".to_owned())?;
    Ok(IpLocationDto {
        city,
        region: non_blank(response.region),
        country: non_blank(response.country),
    })
}

#[tauri::command]
pub async fn infer_ip_location() -> Result<IpLocationDto, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .user_agent("Astro-Agent/0.1")
        .build()
        .map_err(|_| "Could not initialize IP location request".to_owned())?;
    let response = client
        .get(IP_LOCATION_URL)
        .send()
        .await
        .map_err(|_| "Could not reach IP location service".to_owned())?
        .error_for_status()
        .map_err(|_| "IP location service returned an error".to_owned())?;
    let body = response
        .text()
        .await
        .map_err(|_| "Could not read IP location response".to_owned())?;
    parse_ipwho_response(&body)
}

#[cfg(test)]
mod tests {
    use super::parse_ipwho_response;

    #[test]
    fn parses_valid_city() {
        let dto = parse_ipwho_response(
            r#"{"success":true,"city":"Hangzhou","region":"Zhejiang","country":"China"}"#,
        )
        .unwrap();
        assert_eq!(dto.city, "Hangzhou");
        assert_eq!(dto.region.as_deref(), Some("Zhejiang"));
        assert_eq!(dto.country.as_deref(), Some("China"));
    }

    #[test]
    fn rejects_failed_response() {
        let err =
            parse_ipwho_response(r#"{"success":false,"message":"rate limited"}"#).unwrap_err();
        assert_eq!(err, "IP location service rejected the request");
    }

    #[test]
    fn rejects_blank_city() {
        let err = parse_ipwho_response(r#"{"success":true,"city":"  "}"#).unwrap_err();
        assert_eq!(err, "IP location response did not include a city");
    }

    #[test]
    fn rejects_invalid_json() {
        let err = parse_ipwho_response("not json").unwrap_err();
        assert_eq!(err, "Invalid IP location response");
    }
}
