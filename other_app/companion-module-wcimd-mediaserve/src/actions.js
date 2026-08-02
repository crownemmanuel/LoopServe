module.exports = function (self) {
	const choices =
		self.assetChoices && self.assetChoices.length
			? self.assetChoices
			: [{ id: '', label: 'No assets loaded' }]

	self.setActionDefinitions({
		set_live_asset: {
			name: 'Set Live (Asset)',
			options: [
				{
					type: 'dropdown',
					id: 'assetId',
					label: 'Asset',
					default: choices[0]?.id || '',
					choices,
				},
			],
			callback: async (event) => {
				const id = event.options.assetId
				if (!id) {
					self.log('warn', 'No asset selected')
					return
				}
				await self.setLiveById(id)
			},
		},

		set_live_id: {
			name: 'Set Live (ID)',
			options: [
				{
					type: 'textinput',
					id: 'assetId',
					label: 'Asset ID',
					default: '',
					useVariables: true,
				},
			],
			callback: async (event) => {
				const id = String(await self.parseVariablesInString(event.options.assetId || '')).trim()
				if (!id) {
					self.log('warn', 'Asset ID is empty')
					return
				}
				await self.setLiveById(id)
			},
		},

		set_live_name: {
			name: 'Set Live (Name)',
			options: [
				{
					type: 'textinput',
					id: 'name',
					label: 'Asset Name',
					default: '',
					useVariables: true,
				},
			],
			callback: async (event) => {
				const name = String(await self.parseVariablesInString(event.options.name || '')).trim()
				if (!name) {
					self.log('warn', 'Asset name is empty')
					return
				}
				await self.setLiveByName(name)
			},
		},

		trigger_bank: {
			name: 'Trigger Bank',
			description:
				'Fires a LoopServe bank. The bank id never changes — remap which asset it plays in LoopServe → Banks, and this button follows without being edited.',
			options: [
				{
					type: 'textinput',
					id: 'bankId',
					label: 'Bank ID',
					default: '1',
					useVariables: true,
					tooltip: 'Matches the numbered slot in the LoopServe Banks grid.',
				},
			],
			callback: async (event) => {
				const bankId = String(await self.parseVariablesInString(event.options.bankId || '')).trim()
				if (!bankId) {
					self.log('warn', 'Bank ID is empty')
					return
				}
				await self.triggerBank(bankId)
			},
		},

		clear_live: {
			name: 'Clear Live',
			options: [],
			callback: async () => {
				await self.clearLive()
			},
		},

		refresh_assets: {
			name: 'Refresh Assets',
			options: [],
			callback: async () => {
				await self.refreshState()
			},
		},
	})
}
