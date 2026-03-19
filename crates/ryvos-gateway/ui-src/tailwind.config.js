export default {
  content: ['./src/**/*.{svelte,js,ts}', './index.html'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        surface: {
          DEFAULT: '#1A1A1A',
          dark: '#0F0F0F',
          card: '#222222',
          hover: '#2A2A2A',
        },
        border: 'rgba(255,255,255,0.08)',
        accent: '#F07030',
        'accent-hover': '#E06020',
        'accent-pink': '#F03080',
        'accent-amber': '#F0A040',
        'text-warm': '#E8E4E0',
        'text-muted': '#A09890',
      },
      fontFamily: {
        sans: ['Inter', 'system-ui', 'sans-serif'],
        mono: ['JetBrains Mono', 'monospace'],
      },
    },
  },
  plugins: [],
};
