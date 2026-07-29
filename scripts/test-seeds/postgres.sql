-- Fixture schema for integration and e2e tests: two schemas, FK, index,
-- view, and columns covering the value types the grid must render.

CREATE SCHEMA app;

CREATE TABLE app.customers (
  id serial PRIMARY KEY,
  name text NOT NULL,
  email text UNIQUE,
  tags text[] DEFAULT '{}',
  meta jsonb,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE app.orders (
  id serial PRIMARY KEY,
  customer_id integer NOT NULL REFERENCES app.customers (id),
  amount numeric(10, 2) NOT NULL CONSTRAINT orders_amount_positive CHECK (amount > 0),
  placed_at timestamptz NOT NULL DEFAULT now(),
  note text,
  receipt bytea
);

COMMENT ON TABLE app.orders IS 'Customer orders; amounts in the customer''s currency.';
COMMENT ON COLUMN app.orders.receipt IS 'Raw PDF bytes, NULL until issued.';

CREATE INDEX orders_customer_idx ON app.orders (customer_id);

CREATE VIEW app.recent_orders AS
SELECT o.id, c.name AS customer, o.amount, o.placed_at
FROM app.orders o
JOIN app.customers c ON c.id = o.customer_id
ORDER BY o.placed_at DESC;

CREATE MATERIALIZED VIEW app.order_totals AS
SELECT customer_id, sum(amount) AS total
FROM app.orders
GROUP BY customer_id;

CREATE TABLE public.settings (
  key text PRIMARY KEY,
  value jsonb NOT NULL
);

-- No primary key on purpose: exercises the ctid-based editing path.
CREATE TABLE public.audit_log (
  at timestamptz NOT NULL DEFAULT now(),
  message text NOT NULL
);

INSERT INTO app.customers (name, email, tags, meta) VALUES
  ('Ada Lovelace', 'ada@example.com', '{vip,eu}', '{"plan": "pro", "seats": 3}'),
  ('Alan Turing', 'alan@example.com', '{eu}', '{"plan": "free"}'),
  ('Grace Hopper', NULL, '{}', NULL);

INSERT INTO app.orders (customer_id, amount, note, receipt) VALUES
  (1, 129.90, 'first order', decode('deadbeef', 'hex')),
  (1, 49.00, NULL, NULL),
  (2, 999.99, 'wire transfer', NULL);

INSERT INTO public.settings (key, value) VALUES
  ('theme', '"dark"'),
  ('limits', '{"maxRows": 500}');

INSERT INTO public.audit_log (message) VALUES
  ('first entry'),
  ('second entry');

-- Bulk table for streaming / virtual scrolling tests.
CREATE TABLE app.events (
  id serial PRIMARY KEY,
  kind text NOT NULL,
  payload jsonb,
  at timestamptz NOT NULL DEFAULT now()
);

INSERT INTO app.events (kind, payload)
SELECT
  CASE WHEN n % 3 = 0 THEN 'click' WHEN n % 3 = 1 THEN 'view' ELSE 'purchase' END,
  jsonb_build_object('n', n)
FROM generate_series(1, 10000) AS n;
