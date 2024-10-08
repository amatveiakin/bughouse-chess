use censor::Censor;


// Based on:
//   - Censor::Standard + Censor::Sex
//   - https://en.wiktionary.org/wiki/Category:English_swear_words
//   - https://github.com/areebbeigh/profanityfilter/blob/master/profanityfilter/data/badwords.txt
pub fn profanity_censor() -> Censor {
    Censor::custom([
        "arse",
        "arsehole",
        "ass",
        "asshole",
        "bitch",
        "blowjob",
        "bollocks",
        "boob",
        "boobie",
        "boobjob",
        "breast",
        "bugger",
        "clitoris",
        "cock",
        "cocksucker",
        "condom",
        "crap",
        "cum",
        "cunnilingus",
        "cunny",
        "cunt",
        "dick",
        "dickhead",
        "doggystyle",
        "ejaculate",
        "fag",
        "faggot",
        "fagot",
        "felate",
        "felatio",
        "fetish",
        "foreskin",
        "fuck",
        "fucker",
        "handjob",
        "kike",
        "labia",
        "masturbate",
        "masturbation",
        "nigga",
        "nigger",
        "nigra",
        "penis",
        "pimmel",
        "pimpis",
        "piss",
        "prick",
        "pussy",
        "rectum",
        "retard",
        "rimjob",
        "semen",
        "sex",
        "shit",
        "slut",
        "spastic",
        "suck",
        "testes",
        "testicle",
        "testis",
        "tits",
        "tittie",
        "titty",
        "turd",
        "twat",
        "vagina",
        "vulva",
        "wank",
        "wanker",
        "whore",
    ])
}
