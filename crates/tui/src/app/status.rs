//! The status and hints rows: activity, session totals, model and machine stats.

use super::*;

impl App {
    /// Activity in progress, for the left of the status line: the
    /// generation or the initial query to the providers.
    pub fn activity_spans(&self) -> Option<Vec<Span<'static>>> {
        let t = &self.theme;
        let star = Span::styled(
            SPINNER[self.spinner % SPINNER.len()].to_string(),
            t.accent(),
        );
        match &self.gen {
            Generation::Streaming {
                started,
                first_at,
                deltas,
                sent,
                ..
            } => {
                let mut v = vec![star, Span::raw(" ")];
                match first_at {
                    None if started.elapsed() > Duration::from_millis(1500) => {
                        v.extend(self.shimmer("loading model…"));
                        v.push(Span::styled(
                            format!(" ({}) · esc to cancel", fmt_dur(started.elapsed())),
                            t.muted(),
                        ));
                    }
                    None => {
                        v.extend(self.shimmer("thinking…"));
                        v.push(Span::styled(
                            format!(" ({}) · esc to cancel", fmt_dur(started.elapsed())),
                            t.muted(),
                        ));
                    }
                    Some(f) => {
                        let secs = f.elapsed().as_secs_f32().max(0.1);
                        v.extend(self.shimmer("generating"));
                        v.push(Span::styled(
                            format!(
                                " ({} · ↑ ~{} · ↓ {} tokens · ",
                                fmt_dur(started.elapsed()),
                                fmt_k(*sent),
                                fmt_k(*deltas)
                            ),
                            t.muted(),
                        ));
                        v.push(Span::styled(
                            format!("{:.0} tok/s", *deltas as f32 / secs),
                            t.soft(),
                        ));
                        v.push(Span::styled(") · esc to cancel", t.muted()));
                    }
                }
                Some(v)
            }
            Generation::Idle if self.loading => {
                let mut v = vec![star, Span::raw(" ")];
                v.extend(self.shimmer("checking providers…"));
                Some(v)
            }
            Generation::Idle => self.last_run.as_ref().map(|r| self.summary_spans(r)),
        }
    }

    /// Wrap-up of the last generation, in the activity's place:
    /// `✓ done (1m 20s · ↑ 3.2k · ↓ 6.3k tokens · 38 tok/s)`, or `✗ cancelled`
    /// with the same structure if it was cancelled.
    pub(super) fn summary_spans(&self, r: &RunSummary) -> Vec<Span<'static>> {
        let t = &self.theme;
        let head = if r.cancelled {
            Span::styled("✗ cancelled", t.alert())
        } else {
            Span::styled("✓ done", t.ok())
        };
        let tilde = if r.sent_estimated { "~" } else { "" };
        let mut v = vec![
            head,
            Span::styled(
                format!(
                    " ({} · ↑ {tilde}{} · ↓ {} tokens",
                    fmt_dur(r.elapsed),
                    fmt_k(r.sent),
                    fmt_k(r.received)
                ),
                t.muted(),
            ),
        ];
        if let Some(tps) = r.tok_per_s {
            v.push(Span::styled(" · ", t.muted()));
            v.push(Span::styled(format!("{tps:.0} tok/s"), t.soft()));
        }
        v.push(Span::styled(")", t.muted()));
        v
    }

    /// The activity verb with a glint that sweeps it from left to right at
    /// the pace of the star, always in blue: the leading letter in `moon-soft`
    /// and bold, its two neighbors in `moon-soft` and the rest in `moon`.
    /// Between one pass and the next it rests `SHIMMER_REST` ticks.
    pub(super) fn shimmer(&self, word: &str) -> Vec<Span<'static>> {
        let t = &self.theme;
        let period = word.chars().count() + SHIMMER_REST;
        let head = (self.spinner % period) as i64 - 1;
        word.chars()
            .enumerate()
            .map(|(i, c)| {
                let style = match (i as i64 - head).abs() {
                    0 => t.soft_bold(),
                    1 => t.soft(),
                    _ => t.accent(),
                };
                Span::styled(c.to_string(), style)
            })
            .collect()
    }

    /// Context window the conversation really runs in. Ollama does not load
    /// the model with the window it declares but with `num_ctx`, so with it
    /// set the smaller of the two is the truth; every other provider keeps
    /// the one it declares, since `num_ctx` never reaches it.
    pub fn ctx_window(&self) -> Option<u32> {
        let declared = self.ctx_len.filter(|c| *c > 0);
        let asked = self.params.num_ctx.filter(|c| *c > 0);
        if !self.current_is_ollama() {
            return declared;
        }
        match (declared, asked) {
            (Some(d), Some(a)) => Some(d.min(a)),
            (d, a) => d.or(a),
        }
    }

    fn current_is_ollama(&self) -> bool {
        self.current
            .as_ref()
            .and_then(|c| self.providers.iter().find(|p| p.id == c.provider))
            .is_some_and(|p| p.kind == "ollama")
    }

    /// Right of the status line: what is attached, how much of the model's
    /// context window the conversation fills and the session's accumulated
    /// spend.
    pub fn status_spans(&self) -> Vec<Span<'static>> {
        let t = &self.theme;
        let mut v = Vec::new();
        // what is attached has no row of its own any more: the count lives
        // here, and ctrl+f opens the panel that details it
        let n = self.live.len();
        if n > 0 {
            v.push(Span::styled(
                format!("{n} {}", models::plural(n, "file")),
                t.soft(),
            ));
        }
        // what the conversation takes up in the context window: prompt and
        // reply of the last exchange
        let ctx_used = self
            .last_usage
            .as_ref()
            .map(|u| u.prompt_tokens.unwrap_or(0) + u.completion_tokens.unwrap_or(0))
            .unwrap_or(0);
        if ctx_used > 0 {
            dot(&mut v, t);
            match self.ctx_window() {
                Some(ctx) if ctx > 0 => {
                    let pct = (ctx_used as u64 * 100 / ctx as u64) as u32;
                    // from 80 % on the model starts losing the beginning
                    let style = if pct >= 80 { t.soft() } else { t.muted() };
                    v.push(Span::styled(format!("context {pct}%"), style));
                }
                _ => v.push(Span::styled(
                    format!("context {} tok", fmt_k(ctx_used)),
                    t.muted(),
                )),
            }
        }
        // session totals: generation time and tokens, the total and its
        // split between upload and download
        let (up, down) = self.session_tokens_split();
        let time = self.session_time();
        if up + down > 0 || !time.is_zero() {
            let mut inner: Vec<String> = Vec::new();
            if !time.is_zero() {
                inner.push(fmt_dur(time));
            }
            if up + down > 0 {
                inner.push(format!("↑ {}", fmt_k(up)));
                inner.push(format!("↓ {}", fmt_k(down)));
            }
            let body = if up + down > 0 {
                format!("{} tok ({})", fmt_k(up + down), inner.join(" · "))
            } else {
                // only the time: there is nothing to put in parentheses
                inner.join(" · ")
            };
            let sep = if v.is_empty() { "" } else { " · " };
            v.push(Span::styled(format!("{sep}session {body}"), t.muted()));
        }
        v
    }

    /// Generation time accumulated in the session: the duration the provider
    /// gave or, if it gave none, the one measured on screen, for each reply.
    pub fn session_time(&self) -> Duration {
        Duration::from_millis(
            self.messages()
                .filter_map(|m| m.usage.as_ref())
                .filter_map(|u| u.total_duration_ms)
                .sum(),
        )
    }

    /// Active model, for the right of the hints row. Only the name: the
    /// provider is redundant when the model already identifies it.
    pub fn model_spans(&self) -> Vec<Span<'static>> {
        let t = &self.theme;
        match &self.current {
            Some(c) => vec![Span::styled(c.model.clone(), t.muted())],
            None if self.loading => Vec::new(),
            None => vec![Span::styled("no model", t.muted())],
        }
    }

    /// What goes to the right of the model in the hints row: the memory the
    /// model takes up and the machine readings,
    /// `12.1G · cpu 34% ▲61 · ram 57% ▲75`, the current value and the peak of
    /// the last 3 minutes, plus `swap 1.2G` if any. Under 120 columns the
    /// peaks and the size drop out; under 90, everything does.
    pub fn stats_spans(&self, width: u16) -> Vec<Span<'static>> {
        let t = &self.theme;
        if width < STATS_SHORT_MIN_WIDTH {
            return Vec::new();
        }
        let full = width >= STATS_FULL_MIN_WIDTH;
        let mut v: Vec<Span<'static>> = Vec::new();
        // the model in memory: the size showing up is the sign that it is
        // loaded; if it does not all fit in the GPU, how much stays on the CPU
        if full {
            if let LoadedState::Loaded(m) = &self.loaded {
                dot(&mut v, t);
                v.push(Span::styled(
                    format!("{}G", crate::sysmon::fmt_gib(m.size_bytes)),
                    t.muted(),
                ));
                let cpu = m.cpu_percent();
                if cpu > 0 {
                    dot(&mut v, t);
                    v.push(Span::styled(format!("{cpu}% cpu"), t.soft_bold()));
                }
            }
        }
        if let Some(s) = self.sys.current() {
            let pct = |p: f32| {
                if p >= STATS_HOT_PCT {
                    t.soft_bold()
                } else {
                    t.soft()
                }
            };
            dot(&mut v, t);
            v.push(Span::styled("cpu ", t.muted()));
            v.push(Span::styled(format!("{:.0}%", s.cpu), pct(s.cpu)));
            if full {
                v.push(Span::styled(
                    format!(" ▲{:.0}", self.sys.peak_cpu()),
                    t.muted(),
                ));
            }
            v.push(Span::styled(" · ram ", t.muted()));
            v.push(Span::styled(format!("{:.0}%", s.ram), pct(s.ram)));
            if full {
                v.push(Span::styled(
                    format!(" ▲{:.0}", self.sys.peak_ram()),
                    t.muted(),
                ));
            }
            if s.swap_used > 0 {
                v.push(Span::styled(
                    format!(" · swap {}G", crate::sysmon::fmt_gib(s.swap_used)),
                    t.muted(),
                ));
            }
        }
        v
    }

    /// Tokens consumed in the session: sum of prompt and reply of each exchange.
    pub fn session_tokens(&self) -> u32 {
        let (up, down) = self.session_tokens_split();
        up + down
    }

    /// Session tokens: (sent in prompts, received in replies).
    pub fn session_tokens_split(&self) -> (u32, u32) {
        self.messages()
            .filter_map(|m| m.usage.as_ref())
            .fold((0, 0), |(u, d), x| {
                (
                    u + x.prompt_tokens.unwrap_or(0),
                    d + x.completion_tokens.unwrap_or(0),
                )
            })
    }

    /// Footer shortcuts of the open panel, the only place they are listed.
    pub fn panel_keys(&self) -> Vec<(&'static str, &'static str)> {
        match &self.panel {
            Some(Panel::Models(p)) | Some(Panel::Sessions(p)) | Some(Panel::Files(p)) => {
                p.keys.clone()
            }
            Some(Panel::Browse { picker, .. }) => picker.keys.clone(),
            Some(Panel::Help(_)) => {
                vec![("tab", "section"), ("↑↓", "scroll"), ("esc", "close")]
            }
            Some(Panel::Machine) => vec![("esc", "close")],
            Some(Panel::SessionAction { action, .. }) => match action {
                SessionAction::Delete { .. } => {
                    vec![("↑↓", "choose"), ("enter", "confirm"), ("esc", "keep")]
                }
                SessionAction::Rename { .. } => vec![("enter", "save"), ("esc", "cancel")],
            },
            None => Vec::new(),
        }
    }

    /// Hints on the left of the bottom row. While a panel is open the row is
    /// not drawn: the panel carries its own shortcuts in its footer.
    pub fn hints(&self) -> &'static str {
        match self.is_streaming() {
            true => " esc to cancel · ctrl+c to cancel · pgup/pgdn scroll",
            false if self.suggest_navigable() => {
                " ↑↓ choose · tab or enter to complete · esc to clear"
            }
            false if self.suggestions().is_some() => " tab or enter to complete · esc to clear",
            false => " /model switch model · ctrl+s sessions · ctrl+j newline · /help",
        }
    }
}

/// ` · ` separator between pieces of a row, only if something already precedes it.
fn dot(v: &mut Vec<Span<'static>>, t: &Theme) {
    if !v.is_empty() {
        v.push(Span::styled(" · ", t.muted()));
    }
}
