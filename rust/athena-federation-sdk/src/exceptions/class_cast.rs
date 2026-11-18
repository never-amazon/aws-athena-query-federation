const BOOTSTRAP: &str = "bootstrap";
const PLATFORM: &str = "platform";

const JDK_PACKAGES: [(&str, &str, &str); 47] = [
    ("java.lang", "java.base", BOOTSTRAP),
    ("java.lang.instrument", "java.instrument", BOOTSTRAP),
    ("java.lang.management", "java.management", BOOTSTRAP),
    ("java.math", "java.base", BOOTSTRAP),
    ("java.util", "java.base", BOOTSTRAP),
    ("java.util.logging", "java.logging", BOOTSTRAP),
    ("java.util.prefs", "java.prefs", BOOTSTRAP),
    ("java.time", "java.base", BOOTSTRAP),
    ("java.io", "java.base", BOOTSTRAP),
    ("java.nio", "java.base", BOOTSTRAP),
    ("java.net", "java.base", BOOTSTRAP),
    ("java.net.http", "java.net.http", PLATFORM),
    ("java.text", "java.base", BOOTSTRAP),
    ("java.security", "java.base", BOOTSTRAP),
    ("javax.crypto", "java.base", BOOTSTRAP),
    ("javax.net", "java.base", BOOTSTRAP),
    ("javax.security.auth", "java.base", BOOTSTRAP),
    (
        "javax.security.auth.kerberos",
        "java.security.jgss",
        PLATFORM,
    ),
    ("javax.security.cert", "java.base", BOOTSTRAP),
    ("javax.security.sasl", "java.security.sasl", BOOTSTRAP),
    ("org.ietf.jgss", "java.security.jgss", PLATFORM),
    ("java.sql", "java.sql", PLATFORM),
    ("javax.sql", "java.sql", PLATFORM),
    ("javax.sql.rowset", "java.sql.rowset", PLATFORM),
    ("java.awt", "java.desktop", BOOTSTRAP),
    ("java.awt.datatransfer", "java.datatransfer", BOOTSTRAP),
    ("java.beans", "java.desktop", BOOTSTRAP),
    ("javax.swing", "java.desktop", BOOTSTRAP),
    ("javax.imageio", "java.desktop", BOOTSTRAP),
    ("javax.sound", "java.desktop", BOOTSTRAP),
    ("javax.print", "java.desktop", BOOTSTRAP),
    ("javax.naming", "java.naming", BOOTSTRAP),
    ("java.rmi", "java.rmi", BOOTSTRAP),
    ("javax.rmi.ssl", "java.rmi", BOOTSTRAP),
    ("javax.management", "java.management", BOOTSTRAP),
    ("javax.script", "java.scripting", PLATFORM),
    ("javax.lang.model", "java.compiler", PLATFORM),
    ("javax.tools", "java.compiler", PLATFORM),
    ("javax.annotation.processing", "java.compiler", PLATFORM),
    ("javax.xml", "java.xml", BOOTSTRAP),
    ("javax.xml.crypto", "java.xml.crypto", PLATFORM),
    ("org.w3c.dom", "java.xml", BOOTSTRAP),
    ("org.w3c.dom.css", "jdk.xml.dom", PLATFORM),
    ("org.w3c.dom.html", "jdk.xml.dom", PLATFORM),
    ("org.w3c.dom.stylesheets", "jdk.xml.dom", PLATFORM),
    ("org.w3c.dom.xpath", "jdk.xml.dom", PLATFORM),
    ("org.xml.sax", "java.xml", BOOTSTRAP),
];

fn package_matches(package: &str, prefix: &str) -> bool {
    package
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

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
    JDK_PACKAGES
        .iter()
        .filter(|(prefix, _, _)| package_matches(package, prefix))
        .max_by_key(|(prefix, _, _)| prefix.len())
        .map(|(_, module, loader)| (*module, *loader))
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
