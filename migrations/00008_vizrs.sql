-- VizRS graphics engine control (secondary events sent on clip start).
CREATE TABLE config_vizrs (
    config_id INTEGER PRIMARY KEY,
    enable INTEGER NOT NULL DEFAULT 0,
    address TEXT NOT NULL DEFAULT '127.0.0.1:6100',
    token TEXT NOT NULL DEFAULT '',
    on_clip TEXT NOT NULL DEFAULT '',
    on_ad TEXT NOT NULL DEFAULT '',
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

INSERT INTO config_vizrs(config_id)
SELECT id FROM config;
