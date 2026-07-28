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
  amount numeric(10, 2) NOT NULL,
  placed_at timestamptz NOT NULL DEFAULT now(),
  note text,
  receipt bytea
);

CREATE INDEX orders_customer_idx ON app.orders (customer_id);

CREATE VIEW app.recent_orders AS
SELECT o.id, c.name AS customer, o.amount, o.placed_at
FROM app.orders o
JOIN app.customers c ON c.id = o.customer_id
ORDER BY o.placed_at DESC;

CREATE TABLE public.settings (
  key text PRIMARY KEY,
  value jsonb NOT NULL
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
