use fluent::{FluentArgs, FluentBundle, FluentResource};

const EN_US: &str = include_str!("../locales/en-US.ftl");

/// Localized strings. Only English ships in v1; the bundle is the seam for more locales.
pub struct Strings {
    bundle: FluentBundle<FluentResource>,
}

impl Strings {
    pub fn english() -> Result<Self, String> {
        let resource = FluentResource::try_new(EN_US.to_owned()).map_err(|(_, e)| format!("en-US.ftl: {e:?}"))?;
        let lang = "en-US".parse().map_err(|e| format!("{e:?}"))?;
        let mut bundle = FluentBundle::new(vec![lang]);
        // Isolation marks would render as boxes in VR text.
        bundle.set_use_isolating(false);
        bundle.add_resource(resource).map_err(|e| format!("{e:?}"))?;
        Ok(Self { bundle })
    }

    /// Message `id` with string arguments; falls back to the id so a missing string is visible, not fatal.
    pub fn get(&self, id: &str, args: &[(&str, String)]) -> String {
        let Some(pattern) = self.bundle.get_message(id).and_then(|m| m.value()) else {
            return id.to_owned();
        };
        let mut fa = FluentArgs::new();
        for (k, v) in args {
            fa.set(*k, v.clone());
        }
        let mut errors = Vec::new();
        self.bundle.format_pattern(pattern, Some(&fa), &mut errors).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_formats_without_errors() {
        let s = Strings::english().expect("bundle");
        assert_eq!(s.get("wrist-game", &[("fps", "90".into())]), "90 fps");
        assert_eq!(s.get("missing-id", &[]), "missing-id");
    }
}
