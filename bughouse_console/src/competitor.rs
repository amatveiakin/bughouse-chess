#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Competitor {
    User(String),
    Guest(String),
    // Future possibility: `Bot(...)`
}

impl Competitor {
    pub fn name(&self) -> &str {
        match self {
            Competitor::User(name) => name,
            Competitor::Guest(name) => name,
        }
    }

    pub fn into_name(self) -> String {
        match self {
            Competitor::User(name) => name,
            Competitor::Guest(name) => name,
        }
    }

    pub fn as_user(&self) -> anyhow::Result<&str> {
        match self {
            Competitor::User(name) => Ok(name),
            Competitor::Guest(_) => Err(anyhow::Error::msg(format!("Not a user: {self:?}"))),
        }
    }

    pub fn serialize(&self) -> String {
        match self {
            Competitor::User(name) => format!("user/{name}"),
            Competitor::Guest(name) => format!("guest/{name}"),
        }
    }

    pub fn deserialize(s: &str) -> anyhow::Result<Self> {
        let Some((kind, payload)) = s.split_once('/') else {
            return Err(anyhow::Error::msg(format!("Cannot get competitor kind: \"{s}\"")));
        };
        match kind {
            "user" => Ok(Competitor::User(payload.to_owned())),
            "guest" => Ok(Competitor::Guest(payload.to_owned())),
            _ => Err(anyhow::Error::msg(format!("Unknown competitor kind: \"{s}\""))),
        }
    }
}
