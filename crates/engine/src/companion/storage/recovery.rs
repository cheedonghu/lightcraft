//! Transaction reconciliation: callers supply the same logical payload on a
//! storage-only retry. Visibility and durable confirmation are distinct.
use super::*;

pub(super) fn fingerprint(value: &impl Serialize) -> Result<String> {
    struct Hash(lightcraft_preview::Hasher128);
    impl std::io::Write for Hash {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut hash = Hash(lightcraft_preview::Hasher128::new());
    serde_json::to_writer(&mut hash, value).map_err(error)?;
    Ok(hash.0.finish().to_string())
}
pub(super) fn validate(p: &PendingCommit, photo: PhotoId, visible: &Option<Head>) -> Result<()> {
    if p.photo != photo
        || !valid_transaction(&p.transaction_id)
        || p.head.transaction_id != p.transaction_id
        || p.head.manifest != format!("{}/manifest-{}.json", base(photo), p.transaction_id)
        || (visible.as_ref() != Some(&p.head) && *visible != p.old_head)
    {
        return Err(error("unresolved archive transaction; recovery required"));
    }
    Ok(())
}
pub(super) fn prepare(store: &mut dyn Store, photo: PhotoId, epoch: u64, fingerprint: &str) -> Result<(PendingCommit, bool)> {
    prepare_with_epoch(store, photo, epoch, None, fingerprint)
}
pub(super) fn prepare_clear(
    store: &mut dyn Store,
    photo: PhotoId,
    epoch: u64,
    publication_epoch: u64,
    fingerprint: &str,
) -> Result<(PendingCommit, bool)> {
    prepare_with_epoch(store, photo, epoch, Some(publication_epoch), fingerprint)
}
fn prepare_with_epoch(
    store: &mut dyn Store,
    photo: PhotoId,
    epoch: u64,
    publication_epoch: Option<u64>,
    fingerprint: &str,
) -> Result<(PendingCommit, bool)> {
    let unresolved: Option<PendingCommit> = read(store, &format!("{}/pending.json", base(photo)), SMALL)?;
    if unresolved.as_ref().is_some_and(|p| p.publication_epoch.is_some() && p.publication_epoch != publication_epoch) {
        // An old producer cannot alter or acknowledge a new-context intent,
        // even while its failed HEAD still leaves the old prefix visible.
        return Err(error("confirmed conversation replacement is still pending"));
    }
    if unresolved.as_ref().is_some_and(|p| p.receipt.is_some()) {
        pagination::recover_receipt(store, photo)?;
    }
    let visible: Option<Head> = read(store, &format!("{}/HEAD.json", base(photo)), SMALL)?;
    let existing: Option<PendingCommit> = read(store, &format!("{}/pending.json", base(photo)), SMALL)?;
    if let Some(p) = existing {
        validate(&p, photo, &visible)?;
        if p.expected_epoch != epoch {
            return Err(error("conversation epoch changed"));
        }
        if p.content_digest.as_deref() == Some(fingerprint) {
            return Ok((p.clone(), !p.head.manifest_digest.is_empty()));
        }
        // A changed dirty payload may reuse the same unactivated range. No
        // visible immutable object is rewritten, and the highwater never shrinks.
        if p.content_digest.is_some() && visible.as_ref() != Some(&p.head) {
            let mut p = p;
            p.content_digest = Some(fingerprint.into());
            p.head.manifest_digest.clear();
            return Ok((p, false));
        }
        // A completed older receipt/commit cannot be silently replaced by a new
        // generation. Finish it before reserving the next transaction.
        if !p.head.manifest_digest.is_empty() {
            commit(store, &p)?;
        } else if p.content_digest.is_some() {
            // The original transaction's deterministic range is reused while its
            // old HEAD remains visible. Its highwater retains abandoned suffixes.
            let mut p = p;
            p.content_digest = Some(fingerprint.into());
            return Ok((p, false));
        } else {
            return Err(error("incomplete receipt transaction; retry its confirmation first"));
        }
    }
    let old_head = read(store, &format!("{}/HEAD.json", base(photo)), SMALL)?;
    let tx = super::super::LocalRoundId::new().map_err(error)?.as_str().to_owned();
    Ok((
        PendingCommit {
            photo,
            transaction_id: tx.clone(),
            expected_epoch: epoch,
            publication_epoch: None,
            old_head,
            head: Head { manifest: format!("{}/manifest-{tx}.json", base(photo)), transaction_id: tx, manifest_digest: String::new() },
            reserved_files: 0,
            reuse: None,
            revert_override: None,
            content_digest: Some(fingerprint.into()),
            receipt: None,
        },
        false,
    ))
}
