//! The opening movie from the installed game.
#![cfg(windows)]
use std::path::Path;

const MOVIE: &str = "E:/SteamLibrary/steamapps/common/DDDA/nativePC/movie/event/st100ev00.wmv";

#[test]
fn the_opening_movie_decodes_video_and_audio() {
    let path = Path::new(MOVIE);
    if !path.exists() {
        return;
    }
    let mut v = mtmovie::VideoReader::open(path).unwrap();
    let i = v.info;
    eprintln!("{i:?}");
    assert!(i.width >= 640 && i.height >= 360 && i.fps > 20.0 && i.duration > 30.0, "{i:?}");
    let mut last = -1.0;
    for _ in 0..30 {
        let f = v.next().unwrap().expect("a frame");
        assert_eq!(f.bgra.len(), (i.width * i.height * 4) as usize);
        assert!(f.time > last);
        last = f.time;
    }
    let mut a = mtmovie::AudioReader::open(path).unwrap();
    eprintln!("{:?} {:.1} s", a.format, a.duration);
    assert!(a.format.channels >= 1 && a.format.sample_rate >= 22050);
    let chunk = a.next().unwrap().expect("audio");
    assert!(!chunk.samples.is_empty());
}
