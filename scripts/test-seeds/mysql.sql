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
