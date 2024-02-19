let topics = {};

export const PubSub = {
    subscribe: function (topic, listener) {
        if (!topics[topic]) {
            topics[topic] = new Set();
        }
        topics[topic].add(listener);
    },

    unsubscribe: function (topic, listener) {
        if (topics[topic]) {
            topics[topic].delete(listener);
        }
    },

    publish: function (topic, data = {}) {
        if (topics[topic]) {
            for (let listener of topics[topic]) {
                listener(data);
            }
        }
    },
};
