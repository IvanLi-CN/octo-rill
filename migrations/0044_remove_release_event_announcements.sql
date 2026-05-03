DELETE FROM social_activity_events
WHERE kind = 'announcement'
  AND html_url IS NOT NULL
  AND (
    html_url LIKE 'https://github.com/%/releases/%'
    OR html_url LIKE 'http://github.com/%/releases/%'
  );
