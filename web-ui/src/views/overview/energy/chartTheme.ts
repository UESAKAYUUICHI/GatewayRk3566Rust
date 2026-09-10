export const chartText = '#d9ecff'
export const chartMuted = '#87a9c5'
export const chartGrid = '#173b57'
export const palette = ['#1ed5dc', '#2f80ff', '#35d7a6', '#ffcc66', '#ff667d', '#9d7dff']

export function hours24() {
  return Array.from({ length: 24 }, (_, index) => `${String(index).padStart(2, '0')}:00`)
}

export function wave(seed: number, base: number, amplitude: number) {
  return hours24().map((_, index) => {
    const morning = Math.sin((index - 6 + seed) / 24 * Math.PI * 2)
    const evening = Math.sin((index - 17 - seed) / 24 * Math.PI * 2)
    const ripple = Math.sin((index * 1.7 + seed) / 24 * Math.PI * 6)
    return Number(Math.max(0, base + amplitude * (morning * .45 + evening * .55) + ripple * amplitude * .18).toFixed(2))
  })
}

export const commonChart = {
  backgroundColor: 'transparent',
  textStyle: { color: chartText },
  tooltip: {
    trigger: 'axis',
    backgroundColor: 'rgba(7,24,39,.96)',
    borderColor: '#255878',
    textStyle: { color: chartText },
  },
  grid: { left: 54, right: 28, top: 54, bottom: 38 },
}
