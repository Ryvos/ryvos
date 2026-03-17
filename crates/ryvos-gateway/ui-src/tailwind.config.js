export default {
  content: ['./src/**/*.{svelte,js,ts}', './index.html'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        surface: '#111827',
        border: '#1f2937',
        accent: '#818cf8',
        'accent-hover': '#6366f1',
      },
      fontFamily: {
        sans: ['Inter', 'system-ui', 'sans-serif'],
        mono: ['JetBrains Mono', 'monospace'],
      },
    },
  },
  plugins: [],
};
