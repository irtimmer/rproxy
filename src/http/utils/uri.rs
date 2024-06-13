use std::vec;

use hyper::Uri;

use crate::error::Error;

pub trait UriExt {
    fn normalize_path(self) -> Result<Uri, Error>;
}

impl UriExt for Uri {
    fn normalize_path(self) -> Result<Uri, Error> {
        let mut stack = vec![""];
        let mut trailing_slash = false;
        self.path().split('/').for_each(|e| match e {
            "" | "." => trailing_slash = true,
            ".." => {
                trailing_slash = true;
                if stack.len() > 1 {
                    stack.pop();
                }
            }
            _ => {
                trailing_slash = false;
                stack.push(e)
            }
        });
        if trailing_slash {
            stack.push("");
        }
        let path = stack.join("/");
        if path.len() != self.path().len() {
            let path_and_query = match self.query() {
                Some(q) => [&path, q].join("?"),
                None => path
            };
            let mut parts = self.into_parts();
            parts.path_and_query = Some(path_and_query.try_into()?);
            Ok(Uri::from_parts(parts)?)
        } else {
            Ok(self)
        }
    }
}
