WITH ranked_requests AS (
  SELECT
    id,
    ROW_NUMBER() OVER (
      PARTITION BY mode, request_origin, scope_user_id, producer_ref, kind, variant, entity_id, target_lang, source_hash
      ORDER BY updated_at DESC, created_at DESC, id DESC
    ) AS row_rank
  FROM translation_requests
)
DELETE FROM translation_requests
WHERE id IN (
  SELECT id
  FROM ranked_requests
  WHERE row_rank > 1
);

CREATE UNIQUE INDEX idx_translation_requests_request_key
  ON translation_requests(
    mode,
    request_origin,
    scope_user_id,
    producer_ref,
    kind,
    variant,
    entity_id,
    target_lang,
    source_hash
  );
