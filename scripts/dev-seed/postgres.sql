-- Dev seed: a plausible SaaS shape with enough volume to feel real.
-- Re-runnable: drops and recreates the app schema.
--   organizations   2 000
--   users         100 000
--   projects       10 000
--   tasks         300 000
--   invoices      100 000
--   events      1 000 000

\set ON_ERROR_STOP on
\timing on

DROP SCHEMA IF EXISTS app CASCADE;
CREATE SCHEMA app;

CREATE TABLE app.organizations (
  id serial PRIMARY KEY,
  name text NOT NULL,
  plan text NOT NULL,
  seats integer NOT NULL,
  metadata jsonb,
  created_at timestamptz NOT NULL
);

INSERT INTO app.organizations (name, plan, seats, metadata, created_at)
SELECT
  initcap((ARRAY['acme','globex','initech','umbrella','stark','wayne','hooli','piedpiper','massive','dynamic'])[1 + n % 10]
    || ' ' || (ARRAY['labs','corp','industries','systems','software','digital','studio','works'])[1 + n % 8]
    || ' ' || n),
  (ARRAY['free','pro','business','enterprise'])[1 + n % 4],
  5 + n % 200,
  CASE WHEN n % 5 = 0 THEN NULL ELSE jsonb_build_object('industry', (ARRAY['fintech','health','retail','media','gaming'])[1 + n % 5], 'mrr', (n % 900) * 10) END,
  now() - (n % 1000) * interval '1 day'
FROM generate_series(1, 2000) AS n;

CREATE TABLE app.users (
  id serial PRIMARY KEY,
  organization_id integer NOT NULL REFERENCES app.organizations (id),
  email text NOT NULL UNIQUE,
  name text NOT NULL,
  role text NOT NULL,
  active boolean NOT NULL,
  tags text[] NOT NULL DEFAULT '{}',
  prefs jsonb,
  last_login_at timestamptz,
  created_at timestamptz NOT NULL
);

INSERT INTO app.users (organization_id, email, name, role, active, tags, prefs, last_login_at, created_at)
SELECT
  1 + n % 2000,
  'user' || n || '@' || (ARRAY['example.com','mail.dev','corp.io','acme.test'])[1 + n % 4],
  initcap((ARRAY['ada','alan','grace','edsger','barbara','donald','leslie','tony','margaret','john'])[1 + n % 10]
    || ' ' || (ARRAY['lovelace','turing','hopper','dijkstra','liskov','knuth','lamport','hoare','hamilton','backus'])[1 + n % 10]),
  (ARRAY['owner','admin','member','viewer'])[1 + n % 4],
  n % 7 <> 0,
  CASE WHEN n % 3 = 0 THEN ARRAY['beta'] WHEN n % 3 = 1 THEN ARRAY['vip','beta'] ELSE '{}' END,
  CASE WHEN n % 4 = 0 THEN NULL ELSE jsonb_build_object('theme', CASE WHEN n % 2 = 0 THEN 'dark' ELSE 'light' END, 'locale', (ARRAY['en','fr','de','es'])[1 + n % 4]) END,
  CASE WHEN n % 11 = 0 THEN NULL ELSE now() - (n % 90) * interval '1 hour' END,
  now() - (n % 700) * interval '1 day'
FROM generate_series(1, 100000) AS n;

CREATE INDEX users_org_idx ON app.users (organization_id);

CREATE TABLE app.projects (
  id serial PRIMARY KEY,
  organization_id integer NOT NULL REFERENCES app.organizations (id),
  name text NOT NULL,
  status text NOT NULL,
  budget numeric(12, 2),
  archived boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL
);

INSERT INTO app.projects (organization_id, name, status, budget, archived, created_at)
SELECT
  1 + n % 2000,
  initcap((ARRAY['apollo','hermes','atlas','nova','orion','vega','lyra','draco'])[1 + n % 8] || ' ' || (n / 8)),
  (ARRAY['draft','active','paused','done'])[1 + n % 4],
  CASE WHEN n % 6 = 0 THEN NULL ELSE ((n % 5000) * 25)::numeric(12, 2) END,
  n % 9 = 0,
  now() - (n % 400) * interval '1 day'
FROM generate_series(1, 10000) AS n;

CREATE INDEX projects_org_idx ON app.projects (organization_id);

CREATE TABLE app.tasks (
  id serial PRIMARY KEY,
  project_id integer NOT NULL REFERENCES app.projects (id),
  assignee_id integer REFERENCES app.users (id),
  title text NOT NULL,
  state text NOT NULL,
  priority integer NOT NULL,
  estimate_hours numeric(6, 2),
  due_date date,
  created_at timestamptz NOT NULL
);

INSERT INTO app.tasks (project_id, assignee_id, title, state, priority, estimate_hours, due_date, created_at)
SELECT
  1 + n % 10000,
  CASE WHEN n % 8 = 0 THEN NULL ELSE 1 + n % 100000 END,
  (ARRAY['fix','implement','review','design','refactor','document','test','ship'])[1 + n % 8]
    || ' ' || (ARRAY['login flow','billing page','api client','search index','onboarding','exports','webhooks','dashboard'])[1 + n % 8]
    || ' #' || n,
  (ARRAY['todo','in-progress','blocked','done'])[1 + n % 4],
  1 + n % 5,
  CASE WHEN n % 4 = 0 THEN NULL ELSE ((n % 40) + 1)::numeric(6, 2) / 2 END,
  CASE WHEN n % 3 = 0 THEN NULL ELSE current_date + (n % 60 - 30) END,
  now() - (n % 300) * interval '1 day'
FROM generate_series(1, 300000) AS n;

CREATE INDEX tasks_project_idx ON app.tasks (project_id);
CREATE INDEX tasks_assignee_idx ON app.tasks (assignee_id);

CREATE TABLE app.invoices (
  id serial PRIMARY KEY,
  organization_id integer NOT NULL REFERENCES app.organizations (id),
  number text NOT NULL UNIQUE,
  amount numeric(12, 2) NOT NULL,
  currency text NOT NULL,
  paid boolean NOT NULL,
  issued_at timestamptz NOT NULL,
  pdf bytea
);

INSERT INTO app.invoices (organization_id, number, amount, currency, paid, issued_at, pdf)
SELECT
  1 + n % 2000,
  'INV-' || to_char(now() - (n % 730) * interval '1 day', 'YYYY') || '-' || lpad(n::text, 7, '0'),
  ((n % 100000) + 100)::numeric(12, 2) / 100 * 12,
  (ARRAY['EUR','USD','GBP'])[1 + n % 3],
  n % 5 <> 0,
  now() - (n % 730) * interval '1 day',
  CASE WHEN n % 50 = 0 THEN decode(md5(n::text), 'hex') ELSE NULL END
FROM generate_series(1, 100000) AS n;

CREATE INDEX invoices_org_idx ON app.invoices (organization_id);

CREATE TABLE app.events (
  id bigserial PRIMARY KEY,
  user_id integer NOT NULL REFERENCES app.users (id),
  kind text NOT NULL,
  payload jsonb,
  at timestamptz NOT NULL
);

INSERT INTO app.events (user_id, kind, payload, at)
SELECT
  1 + n % 100000,
  (ARRAY['page_view','click','signup','purchase','logout','api_call','export','invite'])[1 + n % 8],
  CASE WHEN n % 10 = 0 THEN NULL ELSE jsonb_build_object('path', '/' || (ARRAY['home','settings','billing','projects','tasks'])[1 + n % 5], 'ms', n % 1500) END,
  now() - (n % 5184000) * interval '1 second'
FROM generate_series(1, 1000000) AS n;

CREATE INDEX events_user_idx ON app.events (user_id);
CREATE INDEX events_at_idx ON app.events (at);

CREATE VIEW app.active_projects AS
SELECT p.id, o.name AS organization, p.name, p.status, p.budget,
       count(t.id) AS open_tasks
FROM app.projects p
JOIN app.organizations o ON o.id = p.organization_id
LEFT JOIN app.tasks t ON t.project_id = p.id AND t.state <> 'done'
WHERE NOT p.archived AND p.status = 'active'
GROUP BY p.id, o.name, p.name, p.status, p.budget;

ANALYZE;
