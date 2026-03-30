export default {
  content: ['./src/**/*.{svelte,js,ts}', './index.html'],
  theme: {
    extend: {
      colors: {
        background: '#FEFCF9',
        surface: '#F7F4F0',
        card: '#FFFFFF',
        border: '#1A1A1A',
        'border-soft': '#E8E4E0',
        'text-primary': '#1A1A1A',
        'text-secondary': '#6B6560',
        'text-muted': '#9B9590',
        accent: '#F07030',
        'accent-hover': '#E06020',
        'accent-light': '#FEF3EC',
        success: '#16A34A',
        warning: '#D97706',
        danger: '#DC2626',
        terminal: '#1A1A1A',
      },
      fontFamily: {
        heading: ['"DM Serif Display"', 'serif'],
        body: ['"Plus Jakarta Sans"', 'sans-serif'],
        mono: ['"JetBrains Mono"', 'monospace'],
      },
      borderRadius: {
        DEFAULT: '0px',
        sm: '0px',
        md: '0px',
        lg: '0px',
        xl: '0px',
        '2xl': '0px',
        full: '9999px',
      },
    },
  },
  plugins: [],
};
