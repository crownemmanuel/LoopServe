const { combineRgb } = require('@companion-module/base')

module.exports = function (self) {
	const choices =
		self.assetChoices && self.assetChoices.length
			? self.assetChoices
			: [{ id: '', label: 'No assets loaded' }]

	self.setFeedbackDefinitions({
		asset_is_live: {
			type: 'boolean',
			name: 'Asset Is Live',
			description: 'True when the selected asset is currently live',
			defaultStyle: {
				bgcolor: combineRgb(251, 191, 36),
				color: combineRgb(0, 0, 0),
			},
			options: [
				{
					type: 'dropdown',
					id: 'assetId',
					label: 'Asset',
					default: choices[0]?.id || '',
					choices,
				},
			],
			callback: (feedback) => {
				return Boolean(feedback.options.assetId) && self.liveId === feedback.options.assetId
			},
		},

		something_is_live: {
			type: 'boolean',
			name: 'Something Is Live',
			description: 'True when any asset is currently live',
			defaultStyle: {
				bgcolor: combineRgb(45, 212, 191),
				color: combineRgb(0, 0, 0),
			},
			options: [],
			callback: () => Boolean(self.liveId),
		},
	})
}
