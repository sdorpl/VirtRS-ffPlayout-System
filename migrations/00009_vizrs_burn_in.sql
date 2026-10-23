-- VizRS /output page burned into the program video (empty = off).
ALTER TABLE config_vizrs
ADD COLUMN burn_url TEXT NOT NULL DEFAULT '';
