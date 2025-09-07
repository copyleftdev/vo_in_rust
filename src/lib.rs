use email_address::EmailAddress;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU8;
use thiserror::Error;
use time::{macros::format_description, OffsetDateTime};

//
// ---------- DTO (untrusted boundary) ----------
//

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserDto {
    pub user_name: Option<String>,
    pub user_age: Option<String>,      // age-as-string (lol)
    pub email_address: Option<String>, // maybe junk
    pub created_at: Option<LooseTime>, // random format
}

// Supports API that sometimes sends a number, sometimes a string
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum LooseTime {
    UnixSecs(i64),
    Rfc3339(String),
}

//
// ---------- Domain (trusted) ----------
//

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub username: Username,
    pub age: Age,
    pub email: Email,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Username(String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Email(EmailAddress);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Age(NonZeroU8);

impl Username {
    pub fn new(s: impl Into<String>) -> Result<Self, AclError> {
        let s = s.into();
        if s.trim().is_empty() { return Err(AclError::UsernameEmpty); }
        Ok(Self(s))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

impl Email {
    pub fn parse(s: &str) -> Result<Self, AclError> {
        EmailAddress::parse(s).map(Self).map_err(|_| AclError::InvalidEmail)
    }
    pub fn as_str(&self) -> &str { self.0.as_str() }
}

impl Age {
    pub fn parse_str(s: &str) -> Result<Self, AclError> {
        let n: u16 = s.trim().parse().map_err(|_| AclError::InvalidAge)?;
        let n = u8::try_from(n).map_err(|_| AclError::InvalidAge)?;
        let nz = NonZeroU8::new(n).ok_or(AclError::InvalidAge)?;
        Ok(Self(nz))
    }
    pub fn get(self) -> u8 { self.0.get() }
}

//
// ---------- ACL Errors ----------
//

#[derive(Debug, Error)]
pub enum AclError {
    #[error("missing field: {0}")]
    Missing(&'static str),
    #[error("username empty")]
    UsernameEmpty,
    #[error("invalid email")]
    InvalidEmail,
    #[error("invalid age")]
    InvalidAge,
    #[error("invalid created_at")]
    InvalidCreatedAt,
}

//
// ---------- ACL (mapping in/out) ----------
//

pub struct Acl;

impl Acl {
    pub fn to_domain(dto: UserDto) -> Result<User, AclError> {
        let username_raw = dto.user_name.ok_or(AclError::Missing("user_name"))?;
        let email_raw = dto.email_address.ok_or(AclError::Missing("email_address"))?;
        let age_raw = dto.user_age.ok_or(AclError::Missing("user_age"))?;
        let created_raw = dto.created_at.ok_or(AclError::Missing("created_at"))?;

        let username = Username::new(username_raw)?;
        let email = Email::parse(&email_raw)?;
        let age = Age::parse_str(&age_raw)?;
        let created_at = parse_loose_time(created_raw)?;

        Ok(User { username, age, email, created_at })
    }

    pub fn to_dto(user: &User) -> UserDto {
        UserDto {
            user_name: Some(user.username.as_str().to_owned()),
            user_age: Some(user.age.get().to_string()),
            email_address: Some(user.email.as_str().to_owned()),
            created_at: Some(LooseTime::UnixSecs(user.created_at.unix_timestamp())),
        }
    }
}

fn parse_loose_time(t: LooseTime) -> Result<OffsetDateTime, AclError> {
    match t {
        LooseTime::UnixSecs(s) => OffsetDateTime::from_unix_timestamp(s).map_err(|_| AclError::InvalidCreatedAt),
        LooseTime::Rfc3339(s) => {
            // try strict RFC3339; fall back to a common format if the API lies
            let rfc = time::format_description::well_known::Rfc3339;
            if let Ok(dt) = OffsetDateTime::parse(&s, &rfc) { return Ok(dt); }
            let alt = format_description!("[year]-[month]-[day] [hour]:[minute]:[second] UTC");
            OffsetDateTime::parse(&s, &alt).map_err(|_| AclError::InvalidCreatedAt)
        }
    }
}

//
// ---------- Discriminated union example for “states” ----------
//

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data")]
pub enum TxnFetch {
    Ok { txns: Vec<String> },
    Empty,
    Err { code: u16, message: String },
}

impl TxnFetch {
    pub fn describe(&self) -> &'static str {
        match self {
            TxnFetch::Ok { .. } => "have_txns",
            TxnFetch::Empty => "no_txns",
            TxnFetch::Err { .. } => "error",
        }
    }
}

//
// ---------- Tests ----------
//

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use time::OffsetDateTime;

    #[test]
    fn dto_to_domain_success_unix() {
        let dto = UserDto {
            user_name: Some("sigma".into()),
            user_age: Some("42".into()),
            email_address: Some("sigma@example.com".into()),
            created_at: Some(LooseTime::UnixSecs(1_700_000_000)),
        };
        let u = Acl::to_domain(dto).unwrap();
        assert_eq!(u.username.as_str(), "sigma");
        assert_eq!(u.age.get(), 42);
        assert_eq!(u.email.as_str(), "sigma@example.com");
        assert!(u.created_at <= OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap());
    }

    #[test]
    fn dto_to_domain_success_rfc3339() {
        let dto = UserDto {
            user_name: Some("sigma".into()),
            user_age: Some("1".into()),
            email_address: Some("sigma@example.com".into()),
            created_at: Some(LooseTime::Rfc3339("2024-12-25T12:34:56Z".into())),
        };
        let u = Acl::to_domain(dto).unwrap();
        assert_eq!(u.created_at.unix_timestamp(), 1735139696);
    }

    #[test]
    fn dto_missing_field_fails() {
        let dto = UserDto {
            user_name: None,
            user_age: Some("10".into()),
            email_address: Some("a@b.co".into()),
            created_at: Some(LooseTime::UnixSecs(0)),
        };
        let err = Acl::to_domain(dto).unwrap_err().to_string();
        assert!(err.contains("missing field: user_name"));
    }

    #[test]
    fn dto_invalid_email_fails() {
        let dto = UserDto {
            user_name: Some("sigma".into()),
            user_age: Some("10".into()),
            email_address: Some("not-an-email".into()),
            created_at: Some(LooseTime::UnixSecs(0)),
        };
        assert!(matches!(Acl::to_domain(dto), Err(AclError::InvalidEmail)));
    }

    #[test]
    fn roundtrip_domain_to_dto() {
        let user = User {
            username: Username::new("sigma").unwrap(),
            age: Age::parse_str("7").unwrap(),
            email: Email::parse("sigma@example.com").unwrap(),
            created_at: OffsetDateTime::from_unix_timestamp(1234567890).unwrap(),
        };
        let dto = Acl::to_dto(&user);
        assert_eq!(dto.user_name.as_deref(), Some("sigma"));
        assert_eq!(dto.user_age.as_deref(), Some("7"));
        assert_eq!(dto.email_address.as_deref(), Some("sigma@example.com"));
        assert_eq!(dto.created_at, Some(LooseTime::UnixSecs(1234567890)));
    }

    #[test]
    fn discriminated_union_states() {
        let ok = TxnFetch::Ok { txns: vec!["t1".into()] };
        let empty = TxnFetch::Empty;
        let err = TxnFetch::Err { code: 503, message: "unavailable".into() };

        assert_eq!(ok.describe(), "have_txns");
        assert_eq!(empty.describe(), "no_txns");
        assert_eq!(err.describe(), "error");

        let s = serde_json::to_string(&ok).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["kind"], json!("Ok"));
    }
}
