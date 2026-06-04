/**
 * RECALL TypeScript SDK — fallback behavior tests.
 *
 * All five methods must return well-formed objects even when the control plane
 * is unreachable. We point at http://localhost:1 to force "connection refused"
 * on every call.
 *
 * Run: npm test
 */

import { RecallClient } from './client'

const DEAD_ENDPOINT = 'http://localhost:1'

async function test_connect() {
  const client = new RecallClient(DEAD_ENDPOINT)
  const ws = await client.connect('test-workspace', {
    agent: 'test-agent',
    model: 'claude-sonnet-4',
  })
  console.assert(ws !== null, 'connect() should return a workspace handle')
  console.log('✓ connect()')
}

async function test_read() {
  const client = new RecallClient(DEAD_ENDPOINT)
  const ws = await client.connect('test-workspace', {
    agent: 'test-agent',
    model: 'gpt-4o',
  })
  const entries = await ws.read({ entity: 'sarah@email.com' })
  console.assert(Array.isArray(entries), 'read() should return an array')
  console.log('✓ read()')
}

async function test_write() {
  const client = new RecallClient(DEAD_ENDPOINT)
  const ws = await client.connect('test-workspace', {
    agent: 'test-agent',
    model: 'gpt-4o',
  })
  const receipt = await ws.write({
    entity: 'sarah@email.com',
    event: 'credit_offered',
    value: '10%',
  })
  console.assert(receipt.id !== null, 'write() should return a receipt with id')
  console.assert(
    receipt.actionKind === 'memory.write',
    'write() receipt should have correct action kind'
  )
  console.log('✓ write()')
}

async function test_handoff() {
  const client = new RecallClient(DEAD_ENDPOINT)
  const capsule = await client.handoff({
    from: 'support-agent',
    to: 'billing-agent',
    entity: 'sarah@email.com',
  })
  console.assert(capsule.id.startsWith('capsule_'), 'handoff() should return capsule with id')
  console.assert(
    capsule.fromAgentId.value === 'support-agent',
    'handoff() from agent should match'
  )
  console.assert(
    capsule.toAgentId.value === 'billing-agent',
    'handoff() to agent should match'
  )
  console.log('✓ handoff()')
}

async function test_publish() {
  const client = new RecallClient(DEAD_ENDPOINT)
  const profile = await client.publish({
    name: 'test-agent',
    version: '1.0',
    description: 'test profile',
  })
  console.assert(profile.name === 'test-agent', 'publish() name should match')
  console.assert(profile.version === '1.0', 'publish() version should match')
  console.assert(profile.immutable === true, 'publish() profile should be immutable')
  console.log('✓ publish()')
}

async function run() {
  try {
    await test_connect()
    await test_read()
    await test_write()
    await test_handoff()
    await test_publish()
    console.log('\n✓ all TypeScript SDK tests passed')
  } catch (err) {
    console.error('✗ test failed:', err)
    process.exit(1)
  }
}

run()
