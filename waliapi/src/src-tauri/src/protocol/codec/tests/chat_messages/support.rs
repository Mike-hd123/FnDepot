use crate::protocol::codec::error::UnsupportedFeatures;

pub(super) fn reject_features(e: &UnsupportedFeatures) -> Vec<String> {
    e.features.clone()
}
