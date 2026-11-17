use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;

pub trait Credentials: Debug + Send + Sync {
    fn get_properties(&self) -> HashMap<String, String>;

    fn as_any(&self) -> &dyn Any;
}
