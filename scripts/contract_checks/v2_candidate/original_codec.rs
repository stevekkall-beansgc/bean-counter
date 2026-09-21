// Test-only codec installed into a disposable approved-core Git archive.
// All model fields derive serde there; this module handles Event's normalized
// and resolved representations without changing or replacing any identity.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedEvent {
    scope: crate::domain::Scope,
    source: String,
    ingress_utf8: String,
    event_utf8: String,
    event_id: String,
    event_hash: String,
    ingress_hash: String,
}
impl serde::Serialize for Event {
    fn serialize<S:serde::Serializer>(&self,s:S)->std::result::Result<S::Ok,S::Error> {
        RetainedEvent {
            scope:self.scope().clone(),source:self.source().into(),
            ingress_utf8:String::from_utf8(self.candidate().ingress_bytes().as_slice().to_vec()).unwrap(),
            event_utf8:String::from_utf8(self.bytes().as_slice().to_vec()).unwrap(),
            event_id:self.id().into(),event_hash:self.content_hash().into(),
            ingress_hash:self.candidate().ingress_hash().into(),
        }.serialize(s)
    }
}
impl<'de> serde::Deserialize<'de> for Event {
    fn deserialize<D:serde::Deserializer<'de>>(d:D)->std::result::Result<Self,D::Error> {
        let wire=RetainedEvent::deserialize(d)?;
        let dto:crate::wire::EventDto=serde_json::from_str(&wire.event_utf8).map_err(serde::de::Error::custom)?;
        let candidate=normalize(wire.ingress_utf8.as_bytes(),wire.scope,&wire.source).map_err(serde::de::Error::custom)?;
        let event=candidate.resolve(dto.chain.as_deref()).map_err(serde::de::Error::custom)?;
        if event.id()!=wire.event_id || event.source()!=wire.source || event.content_hash()!=wire.event_hash
            || event.candidate().ingress_hash()!=wire.ingress_hash
            || event.bytes().as_slice()!=wire.event_utf8.as_bytes()
            || event.candidate().ingress_bytes().as_slice()!=wire.ingress_utf8.as_bytes() {
            return Err(serde::de::Error::custom("original event roundtrip mismatch"));
        }
        Ok(event)
    }
}
