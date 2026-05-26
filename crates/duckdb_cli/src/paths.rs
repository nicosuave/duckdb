fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.trim().is_empty())
}

pub fn home_dir() -> Option<String> {
    env_nonempty("HOME")
        .or_else(|| env_nonempty("USERPROFILE"))
        .or_else(|| {
            let drive = env_nonempty("HOMEDRIVE")?;
            let path = env_nonempty("HOMEPATH")?;
            Some(format!("{}{}", drive, path))
        })
}
