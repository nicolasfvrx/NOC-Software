//! Redemarrage planifie de Firefox a partir du champ `restart_cron`.
//! La planification ne concerne QUE Firefox : NOC Agent, lui, ne
//! redemarre jamais.

use chrono::{DateTime, Local, Timelike};
use cron::Schedule;
use std::str::FromStr;

pub struct CronRestart {
    expression: String,
    schedule: Option<Schedule>,
    next: Option<DateTime<Local>>,
    /// Minute du dernier declenchement : evite plusieurs tirs dans la
    /// meme minute.
    last_fired_minute: Option<DateTime<Local>>,
}

impl CronRestart {
    pub fn new() -> Self {
        Self {
            expression: String::new(),
            schedule: None,
            next: None,
            last_fired_minute: None,
        }
    }

    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// Applique une nouvelle expression cron a chaud (sans redemarrer l'agent).
    /// Retourne true si l'expression a change.
    pub fn update(&mut self, expression: Option<&str>) -> bool {
        let raw = expression.unwrap_or("").trim().to_string();
        if raw == self.expression {
            return false;
        }
        self.expression = raw.clone();
        self.schedule = if raw.is_empty() {
            None
        } else {
            Schedule::from_str(&normalize(&raw)).ok()
        };
        self.next = self.compute_next(Local::now());
        true
    }

    /// Expression fournie mais illisible.
    pub fn is_invalid(&self) -> bool {
        !self.expression.is_empty() && self.schedule.is_none()
    }

    pub fn next_run(&self) -> Option<DateTime<Local>> {
        self.next
    }

    /// A appeler regulierement : true = il faut redemarrer Firefox maintenant.
    pub fn due(&mut self) -> bool {
        let now = Local::now();
        let Some(next) = self.next else {
            return false;
        };
        if now < next {
            return false;
        }

        // Deduplication : un seul declenchement par minute.
        let minute = truncate_to_minute(now);
        let already = self
            .last_fired_minute
            .map(|m| m == minute)
            .unwrap_or(false);

        self.next = self.compute_next(now);
        if already {
            return false;
        }
        self.last_fired_minute = Some(minute);
        true
    }

    fn compute_next(&self, from: DateTime<Local>) -> Option<DateTime<Local>> {
        self.schedule.as_ref()?.after(&from).next()
    }
}

impl Default for CronRestart {
    fn default() -> Self {
        Self::new()
    }
}

fn truncate_to_minute(dt: DateTime<Local>) -> DateTime<Local> {
    dt.with_second(0)
        .and_then(|d| d.with_nanosecond(0))
        .unwrap_or(dt)
}

/// La crate `cron` attend 6 ou 7 champs (secondes en tete).
/// Les expressions classiques a 5 champs ("0 4 * * *") sont converties.
fn normalize(expression: &str) -> String {
    let fields: Vec<&str> = expression.split_whitespace().collect();
    if fields.len() == 5 {
        format!("0 {}", fields.join(" "))
    } else {
        fields.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_field_expression_is_accepted() {
        let mut cron = CronRestart::new();
        assert!(cron.update(Some("0 4 * * *")));
        assert!(!cron.is_invalid());
        assert!(cron.next_run().is_some());
    }

    #[test]
    fn empty_expression_disables_scheduling() {
        let mut cron = CronRestart::new();
        cron.update(Some(""));
        assert!(!cron.is_invalid());
        assert!(cron.next_run().is_none());
    }

    #[test]
    fn invalid_expression_is_reported() {
        let mut cron = CronRestart::new();
        cron.update(Some("pas du cron"));
        assert!(cron.is_invalid());
    }
}
