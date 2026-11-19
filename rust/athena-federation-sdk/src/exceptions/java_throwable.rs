use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::LazyLock;

use crate::exceptions::federation_error::FederationError;
use crate::security::AwsServiceError;

const RUNTIME_EXCEPTION: &str = "java.lang.RuntimeException";
const IO_EXCEPTION: &str = "java.io.IOException";
const ATHENA_CONNECTOR_EXCEPTION: &str =
    "com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException";
const FEDERATION_THROTTLE_EXCEPTION: &str =
    "com.amazonaws.athena.connector.lambda.exceptions.FederationThrottleException";
const AWS_SERVICE_EXCEPTION: &str = "software.amazon.awssdk.awscore.exception.AwsServiceException";

pub trait JavaThrowable: StdError + Send + Sync + 'static {
    fn java_class_name(&self) -> String;

    fn java_message(&self) -> Option<String> {
        Some(self.to_string())
    }

    fn java_cause(&self) -> Option<JavaCause<'_>>;

    fn is_runtime_exception(&self) -> bool {
        true
    }
}

#[derive(Clone, Copy)]
pub enum JavaCause<'a> {
    Throwable(&'a dyn JavaThrowable),
    Foreign(&'a (dyn StdError + 'static)),
}

impl<'a> JavaCause<'a> {
    pub fn foreign(error: &'a (dyn StdError + 'static)) -> JavaCause<'a> {
        JavaCause::Foreign(error)
    }

    pub fn java_class_name(self) -> String {
        match self {
            JavaCause::Throwable(throwable) => throwable.java_class_name(),
            JavaCause::Foreign(error) if error.is::<std::io::Error>() => IO_EXCEPTION.to_string(),
            JavaCause::Foreign(_) => RUNTIME_EXCEPTION.to_string(),
        }
    }

    pub fn java_message(self) -> Option<String> {
        match self {
            JavaCause::Throwable(throwable) => throwable.java_message(),
            JavaCause::Foreign(error) => Some(error.to_string()),
        }
    }

    pub fn java_cause(self) -> Option<JavaCause<'a>> {
        match self {
            JavaCause::Throwable(throwable) => throwable.java_cause(),
            JavaCause::Foreign(error) => error.source().map(JavaCause::foreign),
        }
    }
}

fn foreign_source(error: &dyn StdError) -> Option<JavaCause<'_>> {
    error.source().map(JavaCause::foreign)
}

impl JavaThrowable for FederationError {
    fn java_class_name(&self) -> String {
        let name = match self {
            FederationError::AthenaConnector(_) => ATHENA_CONNECTOR_EXCEPTION,
            FederationError::Throttle(_) => FEDERATION_THROTTLE_EXCEPTION,
            FederationError::IllegalArgument(_) => "java.lang.IllegalArgumentException",
            FederationError::IllegalState(_) | FederationError::UnexpectedToken(_) => {
                "java.lang.IllegalStateException"
            }
            FederationError::UnsupportedOperation(_) => "java.lang.UnsupportedOperationException",
            FederationError::ClassCast(_) => "java.lang.ClassCastException",
            FederationError::NullPointer(_) => "java.lang.NullPointerException",
            FederationError::Arithmetic(_) => "java.lang.ArithmeticException",
            FederationError::IndexOutOfBounds(_) => "java.lang.IndexOutOfBoundsException",
            FederationError::NumberFormat(_) => "java.lang.NumberFormatException",
            FederationError::Io { .. } => IO_EXCEPTION,
            FederationError::Arrow(_)
            | FederationError::Runtime { .. }
            | FederationError::RuntimeWithCause { .. }
            | FederationError::Other(_)
            | FederationError::RuntimeWrapper { .. } => RUNTIME_EXCEPTION,
            FederationError::AwsService(e) => return aws_class_name(e),
            FederationError::External(e) => return e.java_class_name(),
        };
        name.to_string()
    }

    fn java_message(&self) -> Option<String> {
        match self {
            FederationError::NullPointer(None) => None,
            other => Some(other.to_string()),
        }
    }

    fn java_cause(&self) -> Option<JavaCause<'_>> {
        match self {
            FederationError::RuntimeWrapper { source } => {
                Some(JavaCause::Throwable(source.as_ref()))
            }
            FederationError::RuntimeWithCause { cause, .. } => {
                Some(JavaCause::Throwable(cause.as_ref()))
            }
            FederationError::External(e) => e.java_cause(),
            FederationError::Other(e) => foreign_source(e.as_ref()),
            FederationError::Io {
                source: Some(e), ..
            } => foreign_source(e.as_ref()),
            FederationError::Arrow(e) => foreign_source(e.as_ref()),
            FederationError::AthenaConnector(e) => e.cause.as_deref().map(JavaCause::Throwable),
            FederationError::Throttle(e) => e.cause.as_deref().map(JavaCause::Throwable),
            _ => None,
        }
    }

    fn is_runtime_exception(&self) -> bool {
        match self {
            FederationError::Io { .. } => false,
            FederationError::External(e) => e.is_runtime_exception(),
            _ => true,
        }
    }
}

fn aws_class_name(error: &AwsServiceError) -> String {
    error
        .java_class_name()
        .unwrap_or_else(|| AWS_SERVICE_EXCEPTION.to_string())
}

impl JavaThrowable for AwsServiceError {
    fn java_class_name(&self) -> String {
        aws_class_name(self)
    }

    fn java_cause(&self) -> Option<JavaCause<'_>> {
        None
    }
}

const BOOTSTRAP: &str = "bootstrap";

static JDK_PACKAGES: LazyLock<HashMap<&'static str, (&'static str, &'static str)>> =
    LazyLock::new(|| {
        include_str!("jdk17_packages.tsv")
            .lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                Some((fields.next()?, (fields.next()?, fields.next()?)))
            })
            .collect()
    });

fn module_and_loader(name: &str) -> Option<(&'static str, &'static str)> {
    if let Some(element) = name.strip_prefix('[') {
        let element = element.trim_start_matches('[');
        return match element.strip_prefix('L') {
            Some(class) => module_and_loader(class.strip_suffix(';')?),
            None => matches!(element, "Z" | "B" | "C" | "S" | "I" | "J" | "F" | "D")
                .then_some(("java.base", BOOTSTRAP)),
        };
    }
    let (package, class) = name.rsplit_once('.')?;
    if class.is_empty() {
        return None;
    }
    JDK_PACKAGES.get(package).copied()
}

pub fn class_cast_message(from: &str, to: &str) -> String {
    let base = format!("class {from} cannot be cast to class {to}");
    match (module_and_loader(from), module_and_loader(to)) {
        (Some(source), Some(target)) if source == target => {
            let (module, loader) = source;
            format!("{base} ({from} and {to} are in module {module} of loader '{loader}')")
        }
        (Some((from_module, from_loader)), Some((to_module, to_loader))) => format!(
            "{base} ({from} is in module {from_module} of loader '{from_loader}'; {to} is in module {to_module} of loader '{to_loader}')"
        ),
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::class_cast_message;

    #[test]
    fn appends_the_jvm_suffix_when_both_classes_are_in_java_base() {
        assert_eq!(
            class_cast_message("java.lang.String", "java.lang.Integer"),
            "class java.lang.String cannot be cast to class java.lang.Integer (java.lang.String and java.lang.Integer are in module java.base of loader 'bootstrap')"
        );
        assert_eq!(
            class_cast_message("[B", "java.lang.String"),
            "class [B cannot be cast to class java.lang.String ([B and java.lang.String are in module java.base of loader 'bootstrap')"
        );
        assert_eq!(
            class_cast_message("[Ljava.lang.String;", "java.lang.Integer"),
            "class [Ljava.lang.String; cannot be cast to class java.lang.Integer ([Ljava.lang.String; and java.lang.Integer are in module java.base of loader 'bootstrap')"
        );
        assert_eq!(
            class_cast_message("java.util.ImmutableCollections$List12", "java.lang.Integer"),
            "class java.util.ImmutableCollections$List12 cannot be cast to class java.lang.Integer (java.util.ImmutableCollections$List12 and java.lang.Integer are in module java.base of loader 'bootstrap')"
        );
        assert_eq!(
            class_cast_message("[[I", "java.lang.Integer"),
            "class [[I cannot be cast to class java.lang.Integer ([[I and java.lang.Integer are in module java.base of loader 'bootstrap')"
        );
    }

    #[test]
    fn names_each_module_and_loader_for_jdk_platform_modules() {
        assert_eq!(
            class_cast_message("java.lang.String", "java.sql.Date"),
            "class java.lang.String cannot be cast to class java.sql.Date (java.lang.String is in module java.base of loader 'bootstrap'; java.sql.Date is in module java.sql of loader 'platform')"
        );
        assert_eq!(
            class_cast_message("java.util.logging.Level", "java.lang.Integer"),
            "class java.util.logging.Level cannot be cast to class java.lang.Integer (java.util.logging.Level is in module java.logging of loader 'bootstrap'; java.lang.Integer is in module java.base of loader 'bootstrap')"
        );
        assert_eq!(
            class_cast_message("java.sql.Timestamp", "java.lang.Long"),
            "class java.sql.Timestamp cannot be cast to class java.lang.Long (java.sql.Timestamp is in module java.sql of loader 'platform'; java.lang.Long is in module java.base of loader 'bootstrap')"
        );
        assert_eq!(
            class_cast_message("java.sql.Timestamp", "java.sql.Date"),
            "class java.sql.Timestamp cannot be cast to class java.sql.Date (java.sql.Timestamp and java.sql.Date are in module java.sql of loader 'platform')"
        );
        assert_eq!(
            class_cast_message("[Ljava.sql.Date;", "java.lang.Integer"),
            "class [Ljava.sql.Date; cannot be cast to class java.lang.Integer ([Ljava.sql.Date; is in module java.sql of loader 'platform'; java.lang.Integer is in module java.base of loader 'bootstrap')"
        );
        assert_eq!(
            class_cast_message("java.util.logging.Level", "java.sql.Date"),
            "class java.util.logging.Level cannot be cast to class java.sql.Date (java.util.logging.Level is in module java.logging of loader 'bootstrap'; java.sql.Date is in module java.sql of loader 'platform')"
        );
        assert_eq!(
            class_cast_message("java.util.logging.Level", "java.util.logging.Logger"),
            "class java.util.logging.Level cannot be cast to class java.util.logging.Logger (java.util.logging.Level and java.util.logging.Logger are in module java.logging of loader 'bootstrap')"
        );
    }

    #[test]
    fn names_the_java_xml_module_for_dom_and_sax_classes() {
        assert_eq!(
            class_cast_message("org.xml.sax.InputSource", "java.lang.Integer"),
            "class org.xml.sax.InputSource cannot be cast to class java.lang.Integer (org.xml.sax.InputSource is in module java.xml of loader 'bootstrap'; java.lang.Integer is in module java.base of loader 'bootstrap')"
        );
        assert_eq!(
            class_cast_message("org.xml.sax.helpers.AttributesImpl", "org.w3c.dom.Node"),
            "class org.xml.sax.helpers.AttributesImpl cannot be cast to class org.w3c.dom.Node (org.xml.sax.helpers.AttributesImpl and org.w3c.dom.Node are in module java.xml of loader 'bootstrap')"
        );
        assert_eq!(
            class_cast_message("org.w3c.dom.css.CSSRule", "org.w3c.dom.ls.LSInput"),
            "class org.w3c.dom.css.CSSRule cannot be cast to class org.w3c.dom.ls.LSInput (org.w3c.dom.css.CSSRule is in module jdk.xml.dom of loader 'platform'; org.w3c.dom.ls.LSInput is in module java.xml of loader 'bootstrap')"
        );
    }

    #[test]
    fn names_the_jdk_17_module_of_every_package() {
        assert_eq!(870, super::JDK_PACKAGES.len());
        assert_eq!(
            class_cast_message("javax.management.remote.rmi.RMIServerImpl", "java.lang.Integer"),
            "class javax.management.remote.rmi.RMIServerImpl cannot be cast to class java.lang.Integer (javax.management.remote.rmi.RMIServerImpl is in module java.management.rmi of loader 'bootstrap'; java.lang.Integer is in module java.base of loader 'bootstrap')"
        );
        for (class, module, loader) in [
            ("java.applet.Applet", "java.desktop", "bootstrap"),
            (
                "javax.accessibility.Accessible",
                "java.desktop",
                "bootstrap",
            ),
            ("javax.smartcardio.Card", "java.smartcardio", "platform"),
            (
                "javax.transaction.xa.XAResource",
                "java.transaction.xa",
                "platform",
            ),
            ("com.sun.tools.javac.Main", "jdk.compiler", "app"),
            ("sun.awt.AppContext", "java.desktop", "bootstrap"),
        ] {
            assert_eq!(
                class_cast_message(class, "java.lang.Integer"),
                format!("class {class} cannot be cast to class java.lang.Integer ({class} is in module {module} of loader '{loader}'; java.lang.Integer is in module java.base of loader 'bootstrap')"),
            );
        }
        assert_eq!(
            class_cast_message("javax.xml.bind.JAXBElement", "java.lang.Integer"),
            "class javax.xml.bind.JAXBElement cannot be cast to class java.lang.Integer"
        );
    }

    #[test]
    fn leaves_the_suffix_off_when_a_class_has_a_loader_specific_location() {
        assert_eq!(
            class_cast_message("org.apache.calcite.util.TimestampString", "java.lang.Long"),
            "class org.apache.calcite.util.TimestampString cannot be cast to class java.lang.Long"
        );
        assert_eq!(
            class_cast_message("[Lorg.example.Thing;", "java.lang.Integer"),
            "class [Lorg.example.Thing; cannot be cast to class java.lang.Integer"
        );
        assert_eq!(
            class_cast_message("Thing", "java.lang.Integer"),
            "class Thing cannot be cast to class java.lang.Integer"
        );
    }
}
