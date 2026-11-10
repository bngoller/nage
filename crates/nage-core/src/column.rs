//! The SQLite column forms of the core's values (SPEC 7, decisions 57, 89 and 98): `Money` as the
//! INTEGER of its minor units, `Date` and `Timestamp` as their exact text, `Kind` and `SystemKey`
//! as their text, each delegating to sqlx's own `i64`, `str` and `String` impls and reading back
//! strictly. The id newtypes derive their forms where they are defined. No arithmetic here.
#![cfg_attr(not(test), forbid(clippy::arithmetic_side_effects))]

use sqlx::decode::Decode;
use sqlx::encode::{Encode, IsNull};
use sqlx::error::BoxDynError;
use sqlx::sqlite::{SqliteArgumentsBuffer, SqliteTypeInfo, SqliteValueRef};
use sqlx::{Sqlite, Type};

use crate::accounts::{Kind, SystemKey};
use crate::date::Date;
use crate::money::Money;
use crate::timestamp::Timestamp;

impl Type<Sqlite> for Money {
    fn type_info() -> SqliteTypeInfo {
        <i64 as Type<Sqlite>>::type_info()
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        <i64 as Type<Sqlite>>::compatible(ty)
    }
}

impl Encode<'_, Sqlite> for Money {
    /// The minor units as an INTEGER.
    fn encode_by_ref(&self, buf: &mut SqliteArgumentsBuffer) -> Result<IsNull, BoxDynError> {
        <i64 as Encode<'_, Sqlite>>::encode_by_ref(&self.minor(), buf)
    }
}

impl<'r> Decode<'r, Sqlite> for Money {
    /// An INTEGER as minor units.
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        <i64 as Decode<'r, Sqlite>>::decode(value).map(Money::from_minor)
    }
}

impl Type<Sqlite> for Date {
    fn type_info() -> SqliteTypeInfo {
        <str as Type<Sqlite>>::type_info()
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        <str as Type<Sqlite>>::compatible(ty)
    }
}

impl Encode<'_, Sqlite> for Date {
    /// `YYYY-MM-DD` as TEXT.
    fn encode_by_ref(&self, buf: &mut SqliteArgumentsBuffer) -> Result<IsNull, BoxDynError> {
        <String as Encode<'_, Sqlite>>::encode(self.to_string(), buf)
    }
}

impl<'r> Decode<'r, Sqlite> for Date {
    /// TEXT read back by `Date`'s strict parser; another shape or an out-of-range date is a
    /// decode error, never a wrong date.
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        let text = <&'r str as Decode<'r, Sqlite>>::decode(value)?;
        Ok(text.parse::<Date>()?)
    }
}

impl Type<Sqlite> for Timestamp {
    fn type_info() -> SqliteTypeInfo {
        <str as Type<Sqlite>>::type_info()
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        <str as Type<Sqlite>>::compatible(ty)
    }
}

impl Encode<'_, Sqlite> for Timestamp {
    /// `YYYY-MM-DDTHH:MM:SSZ` as TEXT.
    fn encode_by_ref(&self, buf: &mut SqliteArgumentsBuffer) -> Result<IsNull, BoxDynError> {
        <String as Encode<'_, Sqlite>>::encode(self.to_string(), buf)
    }
}

impl<'r> Decode<'r, Sqlite> for Timestamp {
    /// TEXT read back by `Timestamp`'s strict parser; another shape or an out-of-range instant
    /// is a decode error.
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        let text = <&'r str as Decode<'r, Sqlite>>::decode(value)?;
        Ok(text.parse::<Timestamp>()?)
    }
}

impl Type<Sqlite> for Kind {
    fn type_info() -> SqliteTypeInfo {
        <str as Type<Sqlite>>::type_info()
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        <str as Type<Sqlite>>::compatible(ty)
    }
}

impl Encode<'_, Sqlite> for Kind {
    /// The kind's text (`asset`, ...).
    fn encode_by_ref(&self, buf: &mut SqliteArgumentsBuffer) -> Result<IsNull, BoxDynError> {
        <&str as Encode<'_, Sqlite>>::encode_by_ref(&self.as_str(), buf)
    }
}

impl<'r> Decode<'r, Sqlite> for Kind {
    /// One of the five kinds; another text is a decode error.
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        let text = <&'r str as Decode<'r, Sqlite>>::decode(value)?;
        Ok(text.parse::<Kind>()?)
    }
}

impl Type<Sqlite> for SystemKey {
    fn type_info() -> SqliteTypeInfo {
        <str as Type<Sqlite>>::type_info()
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        <str as Type<Sqlite>>::compatible(ty)
    }
}

impl Encode<'_, Sqlite> for SystemKey {
    /// The key's text (`assets`, ..., `uncategorized`).
    fn encode_by_ref(&self, buf: &mut SqliteArgumentsBuffer) -> Result<IsNull, BoxDynError> {
        <&str as Encode<'_, Sqlite>>::encode_by_ref(&self.as_str(), buf)
    }
}

impl<'r> Decode<'r, Sqlite> for SystemKey {
    /// One of the eight keys; another text is a decode error.
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        let text = <&'r str as Decode<'r, Sqlite>>::decode(value)?;
        Ok(text.parse::<SystemKey>()?)
    }
}
