//! The Settings tab: how updates arrive, and the dictionary's languages.
use crate::{Harmonia, library::CHECK_CHOICES, names};
use angelica_sdk::ui::Window;

/// A language's name in itself, as the dictionary shows it.
pub(crate) fn endonym(tag: &str) -> &'static str {
    match tag {
        "ja" => "日本語",
        "de" => "Deutsch",
        "fr" => "Français",
        _ => "English",
    }
}

impl Harmonia {
    fn interval(&self, minutes: u32) -> String {
        match (minutes, self.russian) {
            (60, false) => "every hour".into(),
            (60, true) => "каждый час".into(),
            (1440, false) => "once a day".into(),
            (1440, true) => "раз в день".into(),
            (m, false) if m % 60 == 0 => format!("every {} hours", m / 60),
            (m, true) if m % 60 == 0 => format!("каждые {} ч", m / 60),
            (m, false) => format!("every {m} minutes"),
            (m, true) => format!("каждые {m} мин"),
        }
    }

    pub(crate) fn settings_tab(&self, window: Window) -> Window {
        let options: Vec<String> = CHECK_CHOICES.iter().map(|m| self.interval(*m)).collect();
        #[allow(clippy::cast_possible_truncation)]
        let selected = CHECK_CHOICES
            .iter()
            .position(|m| *m == self.settings.check_minutes)
            .map(|i| i as u32);
        let game = names::tag(self.game_language());
        let mut window = window
            .heading(self.tr("Updates", "Обновления"))
            .checkbox(
                "auto-install",
                self.tr("Install updates by themselves", "Ставить обновления сами"),
                self.settings.auto_install,
            )
            .hint(self.tr(
                "A new version applies at the next game start.",
                "Новая версия применяется при следующем запуске игры.",
            ))
            .choice("interval", self.tr("Check", "Проверять"), options, selected)
            .checkbox(
                "testing",
                self.tr("Testing versions", "Тестовые версии"),
                self.settings.testing,
            )
            .hint(self.tr(
                "They come out earlier, but they can have mistakes.",
                "Выходят раньше, но в них бывают ошибки.",
            ))
            .heading(self.tr("Original in the dictionary", "Оригинал в словаре"));
        for (tag, _) in names::LANGUAGES {
            if tag == game {
                continue;
            }
            window = window.checkbox(
                format!("lang-{tag}"),
                endonym(tag),
                self.settings.dictionary_languages.iter().any(|t| t == tag),
            );
        }
        window
    }

    pub(crate) fn settings_toggled(&mut self, id: &str, checked: bool) {
        match id {
            "auto-install" => self.settings.auto_install = checked,
            "testing" => {
                self.settings.testing = checked;
                // What the feeds offer changes with the channel.
                self.next_check = 0.0;
                self.since_check = 0.0;
            }
            _ => {
                let Some(tag) = id.strip_prefix("lang-") else {
                    return;
                };
                self.settings.dictionary_languages.retain(|t| t != tag);
                if checked {
                    self.settings.dictionary_languages.push(tag.to_owned());
                }
            }
        }
        self.save();
    }

    pub(crate) fn settings_chosen(&mut self, id: &str, index: u32) {
        if id == "interval"
            && let Some(minutes) = CHECK_CHOICES.get(index as usize)
        {
            self.settings.check_minutes = *minutes;
            self.next_check = f64::from(*minutes) * 60.0;
            self.save();
        }
    }
}
