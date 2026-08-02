module.exports = function (self) {
	self.setVariableDefinitions([
		{ variableId: 'live_id', name: 'Live Asset ID' },
		{ variableId: 'live_name', name: 'Live Asset Name' },
		{ variableId: 'live_type', name: 'Live Asset Type' },
		{ variableId: 'asset_count', name: 'Asset Count' },
		{ variableId: 'connection_ok', name: 'Connection OK (true/false)' },
	])
}
