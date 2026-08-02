const { InstanceBase, Regex, runEntrypoint, InstanceStatus } = require('@companion-module/base')
const UpgradeScripts = require('./upgrades')
const UpdateActions = require('./actions')
const UpdateFeedbacks = require('./feedbacks')
const UpdateVariableDefinitions = require('./variables')

class WcimdMediaServeInstance extends InstanceBase {
	constructor(internal) {
		super(internal)

		this.assets = []
		this.assetChoices = []
		this.liveId = null
		this.pollTimer = null
	}

	async init(config) {
		this.config = config
		this.updateStatus(InstanceStatus.Connecting)

		UpdateVariableDefinitions(this)
		this.updateActions()
		this.updateFeedbacks()

		await this.refreshState()
		this.startPolling()
	}

	async destroy() {
		this.stopPolling()
	}

	async configUpdated(config) {
		this.config = config
		this.stopPolling()
		this.updateStatus(InstanceStatus.Connecting)
		await this.refreshState()
		this.startPolling()
	}

	getConfigFields() {
		return [
			{
				type: 'static-text',
				id: 'info',
				label: 'Information',
				width: 12,
				value:
					'Connects to LoopServe (desktop) or WCIMD Media Serve on a Raspberry Pi / FullPageOS host. Default port is 8787. Use the "Trigger Bank" action to fire a numbered bank so remapping happens in LoopServe instead of here.',
			},
			{
				type: 'textinput',
				id: 'host',
				label: 'Host',
				width: 8,
				default: 'wcimdmediaserver.local',
				required: true,
			},
			{
				type: 'textinput',
				id: 'port',
				label: 'Port',
				width: 4,
				default: '8787',
				regex: Regex.PORT,
				required: true,
			},
			{
				type: 'number',
				id: 'pollInterval',
				label: 'Poll Interval (seconds)',
				width: 4,
				default: 5,
				min: 1,
				max: 120,
			},
		]
	}

	baseUrl() {
		const host = (this.config.host || '').trim()
		const port = String(this.config.port || '8787').trim()
		return `http://${host}:${port}`
	}

	startPolling() {
		this.stopPolling()
		const seconds = Number(this.config.pollInterval) || 5
		this.pollTimer = setInterval(() => {
			this.refreshState().catch(() => {})
		}, Math.max(1, seconds) * 1000)
	}

	stopPolling() {
		if (this.pollTimer) {
			clearInterval(this.pollTimer)
			this.pollTimer = null
		}
	}

	async api(path, options = {}) {
		const url = `${this.baseUrl()}${path}`
		const res = await fetch(url, {
			...options,
			headers: {
				Accept: 'application/json',
				...(options.headers || {}),
			},
		})

		let data = null
		const text = await res.text()
		try {
			data = text ? JSON.parse(text) : null
		} catch {
			data = { error: text || 'Invalid JSON response' }
		}

		if (!res.ok) {
			const message = data?.error || `HTTP ${res.status}`
			throw new Error(message)
		}

		return data
	}

	async refreshState() {
		try {
			const data = await this.api('/api/assets')
			this.assets = Array.isArray(data.assets) ? data.assets : []
			this.liveId = data.liveId || null

			const previous = JSON.stringify(this.assetChoices)
			this.assetChoices = this.assets.map((asset) => ({
				id: asset.id,
				label: `${asset.name} (${asset.type})`,
			}))

			const live = this.assets.find((a) => a.id === this.liveId) || null
			this.setVariableValues({
				live_id: live?.id || '',
				live_name: live?.name || '',
				live_type: live?.type || '',
				asset_count: this.assets.length,
				connection_ok: 'true',
			})

			if (previous !== JSON.stringify(this.assetChoices)) {
				this.updateActions()
				this.updateFeedbacks()
			}

			this.checkFeedbacks('asset_is_live', 'something_is_live')
			this.updateStatus(InstanceStatus.Ok)
		} catch (err) {
			this.setVariableValues({ connection_ok: 'false' })
			this.updateStatus(InstanceStatus.ConnectionFailure, err.message || 'Connection failed')
			this.log('error', `WCIMD Media Serve: ${err.message}`)
		}
	}

	async setLiveById(id) {
		try {
			await this.api(`/api/live/${encodeURIComponent(id)}`, { method: 'POST' })
			await this.refreshState()
		} catch (err) {
			this.log('error', `Set live failed: ${err.message}`)
			this.updateStatus(InstanceStatus.ConnectionFailure, err.message)
		}
	}

	async setLiveByName(name) {
		try {
			await this.api('/api/live', {
				method: 'PUT',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({ name }),
			})
			await this.refreshState()
		} catch (err) {
			this.log('error', `Set live by name failed: ${err.message}`)
			this.updateStatus(InstanceStatus.ConnectionFailure, err.message)
		}
	}

	async triggerBank(bankId) {
		try {
			const result = await this.api(`/api/trigger/${encodeURIComponent(bankId)}`, { method: 'POST' })
			// An empty bank is a normal state, not a connection problem.
			if (result && result.fired === false) {
				this.log('warn', result.reason || `Bank ${bankId} is empty`)
			}
			await this.refreshState()
		} catch (err) {
			this.log('error', `Trigger bank ${bankId} failed: ${err.message}`)
			this.updateStatus(InstanceStatus.ConnectionFailure, err.message)
		}
	}

	async clearLive() {
		try {
			await this.api('/api/live/clear', { method: 'POST' })
			await this.refreshState()
		} catch (err) {
			this.log('error', `Clear live failed: ${err.message}`)
			this.updateStatus(InstanceStatus.ConnectionFailure, err.message)
		}
	}

	updateActions() {
		UpdateActions(this)
	}

	updateFeedbacks() {
		UpdateFeedbacks(this)
	}
}

runEntrypoint(WcimdMediaServeInstance, UpgradeScripts)
