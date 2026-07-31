/* global db */
// e2e seed only: soquel_e2e. The Rust integration suites own the soquel_test_*
// databases (created and dropped per test) - never touch them here.
const e2e = db.getSiblingDB('soquel_e2e')

const plans = ['free', 'pro']
const cities = ['Lyon', 'Paris', 'Nantes']
const users = []
for (let i = 0; i < 200; i++) {
  users.push({
    email: `user${i}@example.com`,
    name: `User ${i}`,
    plan: plans[i % 2],
    profile: { city: cities[i % 3], logins: i },
    createdAt: new Date(Date.UTC(2026, 0, 1 + (i % 28))),
  })
}
e2e.users.drop()
e2e.users.insertMany(users)
e2e.users.createIndex({ email: 1 }, { unique: true })

// A view projecting _id away: documents lose their address (read-only in the UI).
e2e.no_id.drop()
e2e.createView('no_id', 'users', [{ $project: { _id: 0, name: 1, plan: 1 } }])

// Delete-test fodder: the spec consumes one per run; reseeds on compose restart.
e2e.disposable.drop()
const disposable = []
for (let i = 0; i < 50; i++)
  disposable.push({ _id: `delete-me-${String(i).padStart(2, '0')}`, note: 'e2e delete fodder' })
e2e.disposable.insertMany(disposable)

print(`soquel_e2e: ${e2e.users.countDocuments()} users, ${e2e.disposable.countDocuments()} disposable`)
