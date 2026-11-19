macro_rules! named_enum {
    ($vis:vis enum $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis enum $name {
            $($variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }

            pub fn from_name(name: &str) -> Option<$name> {
                $name::ALL.iter().copied().find(|v| v.as_str() == name)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

pub(crate) use named_enum;
