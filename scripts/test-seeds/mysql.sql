-- Fixture schema for mysql integration tests; runs inside soquel_test.
-- Mirrors the postgres seed shapes where mysql has an equivalent.

CREATE TABLE customers (
  id INT AUTO_INCREMENT PRIMARY KEY,
  name TEXT NOT NULL,
  email VARCHAR(255) UNIQUE,
  meta JSON,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO customers (name, email, meta) VALUES
  ('Ada Lovelace', 'ada@example.com', '{"plan": "pro", "seats": 3}'),
  ('Alan Turing', 'alan@example.com', '{"plan": "free"}'),
  ('Grace Hopper', NULL, NULL);

CREATE TABLE orders (
  id INT AUTO_INCREMENT PRIMARY KEY,
  customer_id INT NOT NULL,
  amount DECIMAL(10, 2) NOT NULL,
  placed_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
  note TEXT,
  receipt BLOB,
  CONSTRAINT orders_customer_fk FOREIGN KEY (customer_id) REFERENCES customers (id)
);

INSERT INTO orders (customer_id, amount, note, receipt) VALUES
  (1, 129.90, 'first order', X'DEADBEEF'),
  (1, 49.00, NULL, NULL),
  (2, 999.99, 'wire transfer', NULL);

-- Composite key + composite FK: exercises the hand-rolled FK assembly.
CREATE TABLE plans (
  org_id INT NOT NULL,
  code VARCHAR(32) NOT NULL,
  label TEXT,
  PRIMARY KEY (org_id, code)
);

CREATE TABLE subscriptions (
  id INT AUTO_INCREMENT PRIMARY KEY,
  org_id INT NOT NULL,
  plan_code VARCHAR(32) NOT NULL,
  CONSTRAINT subscriptions_plan_fk FOREIGN KEY (org_id, plan_code) REFERENCES plans (org_id, code)
);

INSERT INTO plans (org_id, code, label) VALUES (1, 'pro', 'Pro'), (1, 'free', 'Free');
INSERT INTO subscriptions (org_id, plan_code) VALUES (1, 'pro');

CREATE VIEW recent_orders AS
SELECT o.id, c.name AS customer, o.amount, o.placed_at
FROM orders o
JOIN customers c ON c.id = o.customer_id
ORDER BY o.placed_at DESC;

-- Bulk table for streaming tests in the browse round.
CREATE TABLE events (
  id INT AUTO_INCREMENT PRIMARY KEY,
  kind VARCHAR(32) NOT NULL,
  n INT NOT NULL
);

INSERT INTO events (kind, n)
WITH RECURSIVE seq AS (
  SELECT 1 AS n
  UNION ALL
  SELECT n + 1 FROM seq WHERE n < 1000
)
SELECT CASE WHEN n % 2 = 0 THEN 'click' ELSE 'view' END, n FROM seq;

-- Second database: the snapshot groups per schema, one is never enough.
CREATE DATABASE soquel_other;
GRANT ALL ON soquel_other.* TO 'soquel'@'%';
CREATE TABLE soquel_other.notes (
  id INT AUTO_INCREMENT PRIMARY KEY,
  body TEXT
);
INSERT INTO soquel_other.notes (body) VALUES ('from the other side');
