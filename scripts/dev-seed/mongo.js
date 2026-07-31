/* global db, ObjectId, NumberDecimal, UUID */
// Dev seed: SaaS-shaped documents mirroring the sql dev seeds. Re-runnable:
// drops soquel_dev first.
//   users      5 000  nested profiles, plans, signup dates, unique email index
//   orders    20 000  user refs, Decimal128 amounts, {userId, createdAt} index
//   events    50 000  heterogeneous shapes per type, {type, at} index
//   sessions  10 000  UUID tokens (BinData) for the $binary rendering
//   webhooks      25
const dev = db.getSiblingDB('soquel_dev')
dev.dropDatabase()

const DAY = 86_400_000
const start = new Date('2024-01-01T00:00:00Z').getTime()
const plans = ['free', 'pro', 'team', 'enterprise']
const cities = ['Lyon', 'Paris', 'Nantes', 'Berlin', 'Madrid', 'Austin']

function batchInsert(collection, total, make) {
  const batch = []
  for (let i = 0; i < total; i++) {
    batch.push(make(i))
    if (batch.length === 2000) {
      collection.insertMany(batch)
      batch.length = 0
    }
  }
  if (batch.length > 0)
    collection.insertMany(batch)
}

const userIds = []
batchInsert(dev.users, 5000, (i) => {
  const _id = new ObjectId()
  userIds.push(_id)
  return {
    _id,
    email: `user${i}@example.com`,
    name: `User ${i}`,
    plan: plans[i % plans.length],
    profile: { city: cities[i % cities.length], timezone: 'Europe/Paris', logins: i % 400 },
    tags: i % 7 === 0 ? ['beta', 'newsletter'] : ['newsletter'],
    createdAt: new Date(start + (i % 500) * DAY),
  }
})
dev.users.createIndex({ email: 1 }, { unique: true })

const statuses = ['pending', 'paid', 'shipped', 'refunded']
batchInsert(dev.orders, 20000, i => ({
  userId: userIds[i % userIds.length],
  status: statuses[i % statuses.length],
  amount: NumberDecimal(((i % 900) * 13.37 / 100).toFixed(2)),
  items: (i % 3) + 1,
  createdAt: new Date(start + (i % 550) * DAY),
}))
dev.orders.createIndex({ userId: 1, createdAt: -1 })

const types = ['page_view', 'api_call', 'export', 'login']
batchInsert(dev.events, 50000, (i) => {
  const type = types[i % types.length]
  const base = { type, at: new Date(start + i * 60_000), userId: userIds[i % userIds.length] }
  if (type === 'page_view')
    return { ...base, path: `/app/page-${i % 40}` }
  if (type === 'api_call')
    return { ...base, endpoint: `/v1/resource/${i % 12}`, ms: i % 900 }
  if (type === 'export')
    return { ...base, rows: (i % 100) * 1000, format: i % 2 === 0 ? 'csv' : 'json' }
  return { ...base, ip: `10.0.${i % 255}.${(i * 7) % 255}` }
})
dev.events.createIndex({ type: 1, at: -1 })

batchInsert(dev.sessions, 10000, i => ({
  userId: userIds[i % userIds.length],
  token: UUID(),
  ua: i % 3 === 0 ? 'Mozilla/5.0 (Macintosh)' : 'Mozilla/5.0 (Windows NT 10.0)',
  expiresAt: new Date(start + 600 * DAY + (i % 30) * DAY),
}))

batchInsert(dev.webhooks, 25, i => ({
  url: `https://hooks.example.com/endpoint-${i}`,
  events: i % 2 === 0 ? ['order.paid'] : ['order.paid', 'user.created'],
  active: i % 5 !== 0,
  secretHash: `sha256:${i.toString(16).padStart(8, '0')}`,
}))

print(`soquel_dev: ${dev.users.estimatedDocumentCount()} users, ${dev.orders.estimatedDocumentCount()} orders, ${dev.events.estimatedDocumentCount()} events, ${dev.sessions.estimatedDocumentCount()} sessions, ${dev.webhooks.estimatedDocumentCount()} webhooks`)
