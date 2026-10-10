//! Authoritative project co-op death policy (head7481/8180), not native MP.
//! Never infer game-over/spectator selection from replicated HP or motion.
use ddhost::death::{Report, SeatReport};
use ddid::{Reader, Result, Writer};

fn validate(report: &Report) -> Result<()> {
    for (index, seat) in report.seats.iter().enumerate() {
        if report.seats[..index]
            .iter()
            .any(|old| old.seat == seat.seat || old.profile == seat.profile)
        {
            return Err("duplicate death-report seat/profile".into());
        }
        if let Some(target) = seat.spectator {
            if !seat.terminal
                || target == seat.seat
                || !report
                    .seats
                    .iter()
                    .any(|other| other.seat == target && !other.terminal)
            {
                return Err("invalid death-report spectator".into());
            }
        }
    }
    if report.game_over && (report.seats.is_empty() || report.seats.iter().any(|s| !s.terminal)) {
        return Err("invalid death-report game over".into());
    }
    Ok(())
}

pub fn write(w: &mut Writer, report: &Report, base: Option<&Report>) {
    let changed = base != Some(report);
    w.bool(changed);
    if !changed {
        return;
    }
    w.bool(report.game_over).len(report.seats.len());
    for seat in &report.seats {
        w.u8(seat.seat)
            .u64(seat.profile.0)
            .bool(seat.terminal)
            .bool(seat.spectator.is_some());
        if let Some(target) = seat.spectator {
            w.u8(target);
        }
        w.bool(seat.unsupported.is_some());
        if let Some(error) = &seat.unsupported {
            w.bytes(error.as_bytes());
        }
    }
}

pub fn read(r: &mut Reader, base: Option<&Report>) -> Result<Report> {
    let report = if r.bool()? {
        let game_over = r.bool()?;
        let mut seats = Vec::new();
        for _ in 0..r.len(12)? {
            let seat = r.u8()?;
            let profile = ddhost::ProfileId(r.u64()?);
            let terminal = r.bool()?;
            let spectator = if r.bool()? { Some(r.u8()?) } else { None };
            let unsupported = if r.bool()? {
                Some(String::from_utf8(r.bytes()?).map_err(|e| e.to_string())?)
            } else {
                None
            };
            seats.push(SeatReport {
                seat,
                profile,
                terminal,
                spectator,
                unsupported,
            });
        }
        Report { game_over, seats }
    } else {
        base.cloned().ok_or("death-report baseline absent")?
    };
    validate(&report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report() -> Report {
        Report {
            game_over: false,
            seats: vec![
                SeatReport {
                    seat: 0,
                    profile: ddhost::ProfileId(100),
                    terminal: true,
                    spectator: Some(1),
                    unsupported: Some("untranslated airborne branch".into()),
                },
                SeatReport {
                    seat: 1,
                    profile: ddhost::ProfileId(200),
                    terminal: false,
                    spectator: None,
                    unsupported: None,
                },
            ],
        }
    }
    #[test]
    fn death_report_whole_delta_explicit_game_over_and_end_roundtrip() {
        let first = report();
        let mut w = Writer::new();
        write(&mut w, &first, None);
        assert_eq!(read(&mut Reader::new(&w.finish()), None).unwrap(), first);
        let mut w = Writer::new();
        write(&mut w, &first, Some(&first));
        let unchanged = w.finish();
        assert_eq!(unchanged, vec![0]);
        assert!(read(&mut Reader::new(&unchanged), None).is_err());
        assert_eq!(read(&mut Reader::new(&unchanged), Some(&first)).unwrap(), first);
        let game_over = Report {
            game_over: true,
            seats: first
                .seats
                .iter()
                .cloned()
                .map(|mut s| {
                    s.terminal = true;
                    s.spectator = None;
                    s.unsupported = None;
                    s
                })
                .collect(),
        };
        let mut w = Writer::new();
        write(&mut w, &game_over, Some(&first));
        assert_eq!(
            read(&mut Reader::new(&w.finish()), Some(&first)).unwrap(),
            game_over
        );
        let end = Report::default();
        let mut w = Writer::new();
        write(&mut w, &end, Some(&game_over));
        assert_eq!(
            read(&mut Reader::new(&w.finish()), Some(&game_over)).unwrap(),
            end
        );
    }
    #[test]
    fn death_report_rejects_duplicate_identity_bad_spectator_and_false_game_over() {
        let first = report();
        let mut invalid = Vec::new();
        let mut value = first.clone();
        value.seats[1].seat = 0;
        invalid.push(value);
        let mut value = first.clone();
        value.seats[1].profile = ddhost::ProfileId(100);
        invalid.push(value);
        let mut value = first.clone();
        value.seats[0].spectator = Some(0);
        invalid.push(value);
        let mut value = first.clone();
        value.seats[0].spectator = Some(4);
        invalid.push(value);
        let mut value = first.clone();
        value.seats[1].terminal = true;
        invalid.push(value);
        let mut value = first.clone();
        value.game_over = true;
        invalid.push(value);
        for value in invalid {
            let mut w = Writer::new();
            write(&mut w, &value, None);
            assert!(read(&mut Reader::new(&w.finish()), None).is_err());
        }
        let mut w = Writer::new();
        write(&mut w, &first, None);
        let bytes = w.finish();
        for cut in 0..bytes.len() {
            assert!(read(&mut Reader::new(&bytes[..cut]), None).is_err());
        }
    }
}
