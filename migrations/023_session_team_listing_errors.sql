-- Remember which configured user orgs could not be listed at login (e.g.
-- SAML SSO authorization missing), so the UI can point the user at the fix.
ALTER TABLE sessions
    ADD COLUMN team_listing_errors JSONB NOT NULL DEFAULT '[]'::jsonb;
