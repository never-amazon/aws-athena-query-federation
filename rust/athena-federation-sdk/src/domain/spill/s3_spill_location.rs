use std::fmt;

const SEPARATOR: &str = "/";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct S3SpillLocation {
    bucket: String,
    key: String,
    directory: bool,
}

impl S3SpillLocation {
    pub fn new(
        bucket: impl Into<String>,
        key: impl Into<String>,
        directory: bool,
    ) -> S3SpillLocation {
        S3SpillLocation {
            bucket: bucket.into(),
            key: key.into(),
            directory,
        }
    }

    pub fn get_bucket(&self) -> &str {
        &self.bucket
    }

    pub fn get_key(&self) -> &str {
        &self.key
    }

    pub fn is_directory(&self) -> bool {
        self.directory
    }

    pub fn new_builder() -> S3SpillLocationBuilder {
        S3SpillLocationBuilder::default()
    }
}

impl fmt::Display for S3SpillLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "S3SpillLocation{{bucket='{}', key='{}', directory={}}}",
            self.bucket, self.key, self.directory
        )
    }
}

#[derive(Debug, Clone)]
pub struct S3SpillLocationBuilder {
    bucket: Option<String>,
    prefix: Option<String>,
    query_id: Option<String>,
    split_id: Option<String>,
    is_directory: bool,
}

impl Default for S3SpillLocationBuilder {
    fn default() -> Self {
        S3SpillLocationBuilder {
            bucket: None,
            prefix: None,
            query_id: None,
            split_id: None,
            is_directory: true,
        }
    }
}

fn or_null(v: Option<&str>) -> &str {
    v.unwrap_or("null")
}

impl S3SpillLocationBuilder {
    #[must_use]
    pub fn with_bucket(mut self, bucket: impl Into<String>) -> Self {
        self.bucket = Some(bucket.into());
        self
    }

    #[must_use]
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    #[must_use]
    pub fn with_is_directory(mut self, is_directory: bool) -> Self {
        self.is_directory = is_directory;
        self
    }

    #[must_use]
    pub fn with_query_id(mut self, query_id: impl Into<String>) -> Self {
        self.query_id = Some(query_id.into());
        self
    }

    #[must_use]
    pub fn with_split_id(mut self, split_id: impl Into<String>) -> Self {
        self.split_id = Some(split_id.into());
        self
    }

    pub fn is_directory(&self) -> bool {
        self.is_directory
    }

    pub fn build(&self) -> S3SpillLocation {
        let query_blank = self.query_id.as_deref().is_none_or(|q| q.trim().is_empty());
        let bucket_prefix = if query_blank {
            or_null(self.prefix.as_deref()).to_string()
        } else {
            format!(
                "{}{}{}",
                or_null(self.prefix.as_deref()),
                SEPARATOR,
                or_null(self.query_id.as_deref())
            )
        };
        let key = format!(
            "{}{}{}",
            bucket_prefix,
            SEPARATOR,
            or_null(self.split_id.as_deref())
        );
        S3SpillLocation::new(or_null(self.bucket.as_deref()), key, true)
    }
}
