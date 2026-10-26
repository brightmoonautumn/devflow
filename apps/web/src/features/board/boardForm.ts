import type { BoardFeature } from './boardModel'

/** Radix option values must be nonempty; keep the domain's empty feature at the UI boundary. */
export function featureChoices(features: BoardFeature[]) {
  let unclassified = '__devflow_unclassified__'
  while (features.some(feature => feature.id === unclassified)) unclassified += '_'
  const options = features.map(feature => ({ value: feature.id, label: feature.name }))
  return { unclassified, options, withUnclassified: [{ value: unclassified, label: '未归类' }, ...options] }
}

export const priorityChoices = [{ value: '普通', label: '普通' }, { value: '高', label: '高' }]
